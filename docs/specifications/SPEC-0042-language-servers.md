# SPEC-0042: Language server registry

- **Version:** 21 · **Date:** 2026-11-14 (v21: definitions, references, type definitions and
  implementations get the wait they are asked with, behaviour 37, AK-38; v20: a question to kotlin-language-server waits for its
  diagnostics of the file, behaviour 35; intelephense's second list of an opened file is the answer,
  behaviour 36; v19: intelephense is ready also by its own notification and when it read
  its stored index, behaviour 25; v18: bash-language-server gets the opened text again after it has
  read the saved files, behaviour 34; v17: next to the reference, OBS-0039 W1 to W3: a class or id of an HTML
  file leads to its rules in the stylesheets, a jump link to the element with that id, a class or id selector lists
  its uses in HTML files, and the structure of an HTML file is nested and names id and classes, all from IDECraft's
  own syntax trees, behaviours 28–33; v16: found by asking every language feature of small Go, Rust, C, C++, PHP,
  HTML and CSS projects without a window: rustup installs nothing and its message names the toolchain, clangd is asked after it has
  parsed the file, intelephense after its indexing, the CSS server gets its validation settings, and rust-analyzer's
  diagnostics wait for its background check, behaviours 23–27; v15: a rustup proxy is checked in the project folder, whose toolchain decides whether the component exists, behaviour 2a, headless run for F-039; v14, 2026-11-12: pyright and basedpyright are asked only after they have listed the project's files, so Find Usages, references and rename cover the other files from the first question on, OBS-0034 L8, behaviour 22; v13: TypeScript 7's server is asked for a file's diagnostics, found by the server CI; v12: TypeScript 7, which has no `tsserver.js`, is recognised and its own language server started, or what is missing is said, behaviour 21; v11: files changed on disk outside IDECraft reach the running
  servers, OBS-0027 L35; v10: typescript-language-server without its syntax-only
  server, OBS-0028 item 5; v2: waiting for servers that load the workspace first; v3: TypeScript
  fallback, all language features checked with pyright, typescript-language-server, gopls, intelephense,
  vscode-langservers-extracted and jdtls; v4: servers for XML, YAML, Markdown, TOML, Shell, Dockerfile,
  Groovy, Scala, SCSS, Less, Swift, Lua and R with their quirks; v5: Kotlin with kotlin-language-server
  checked; v6: Ruby with the project's Ruby and Bundler, SPEC-0185; v7: single Java files without a build,
  a missing server named in the status bar, hand test round 5 Nr. 12 and 16; v8: source roots of a
  Java project without a build file, OBS-0011 item 2; v9: jdtls workspace data per project, renewed
  when the inferred source roots change, OBS-0013 item 3)
- **Sources:** GEN (IDEs bring language support for many languages and tell the user what is missing; F-038),
  STD (the language servers' documented command lines: `clangd`, `rust-analyzer`, `basedpyright-langserver
  --stdio`, `pyright-langserver --stdio`, `pylsp`, `typescript-language-server --stdio`, `vtsls --stdio`,
  `gopls`, `jdtls`, `kotlin-language-server`, `intelephense --stdio`, `phpactor language-server`,
  `ruby-lsp`, `solargraph stdio`, `vscode-{html,css,json}-language-server --stdio`; rustup proxies and
  `rustup which`), SPEC-0023 (LSP client); v7: STD (jdtls imports a folder without Maven or Gradle
  files as an invisible project whose source folder is inferred from the opened file), GEN (IDEs
  analyse a single Java file outside any build and name a missing language plugin or tool when such
  a file opens); v8: OBS-0011 item 2 (without a build file the reference treats `src/main/java` as
  source folder and reports no package problem), STD (jdtls setting `java.project.sourcePaths`:
  source folders of a folder without Maven or Gradle files, relative to that folder; the Maven and
  Gradle convention folders `src/main/java` and `src/test/java`), GEN (a source root is the folder
  from which the folders of a file equal its declared package); v9: OBS-0013 item 3 (a project
  opened before v8 kept the package error, a fresh copy did not), STD (jdtls keeps the projects it
  imported in the workspace data folder given with `-data` and reuses them on the next start; its
  launcher derives the default folder from the name of the working folder only); v11: OBS-0027 L35
  (a Java class written to disk by another program stayed unknown to jdtls until it was opened, so
  the type and method hierarchies missed it), STD (LSP `workspace/didChangeWatchedFiles` with the
  change types 1 created, 2 changed, 3 deleted; a client announces
  `workspace.didChangeWatchedFiles.dynamicRegistration` only when it honours the watchers a server
  registers), GEN (IDEs notice files that a version-control checkout, a generator or another editor
  wrote), SPEC-0026 (snapshot comparison); v14: OBS-0034 item 38 (Find Usages on a Python class
  found one usage, in the declaring file, where the reference lists the usages in four files, while
  the rename preview covered all four), OSS (pyright 1.1.414, MIT, measured over the protocol: after
  `initialize` it asks for its settings, then lists the workspace's source files and logs their
  number, then publishes the diagnostics of the opened file; `textDocument/references` and
  `textDocument/rename` answered before the listing cover the opened file only, the same requests
  after it all files), STD (LSP `textDocument/publishDiagnostics`); v15: headless finding of
  2026-11-13 (`format_file` on a Rust file of a project outside the IDECraft repository answered
  "the language server stopped: error: Unknown binary 'rust-analyzer' in official toolchain …",
  because the check of behaviour 2 had run in another folder than the server), STD (rustup: a
  proxy runs the program of the toolchain selected for the folder it is started in —
  `rust-toolchain.toml` or `rust-toolchain` there or above, a folder override, else the default
  toolchain — and `rustup which <program>` answers for the folder it is run in); v16: STD (rustup chooses the
  toolchain for the folder a command runs in, from `rust-toolchain.toml`, `rust-toolchain` or a
  directory override, and its proxies run that toolchain's program; `rustup which <program>` names
  the program of that toolchain or fails; the variable `RUSTUP_AUTO_INSTALL=0` keeps rustup from
  installing a toolchain a folder asks for; LSP work-done progress `$/progress` with `begin`, `report`
  and `end` under a token; `workspace/configuration`), OSS (measured over the protocol with small
  projects, each server started fresh: rustup 1.29.2 with a default toolchain without rust-analyzer
  and a folder whose `rust-toolchain.toml` names a toolchain with it; Apple clangd 21 — a completion
  asked right after `didOpen` is answered from the words of the file, every item of kind `Text`, and
  after the file's first diagnostics with the members, their kinds and types; intelephense 1.18.5 —
  after `initialized` it publishes empty diagnostics for the opened file, reports a progress titled
  "Intelephense (1.18.5) indexing" under a token of its own, answers hover with nothing until that
  progress ends, and publishes the file's diagnostics again right after it; its free edition lists
  `renameProvider`, `implementationProvider`, `typeDefinitionProvider`, `codeActionProvider` and
  `typeHierarchyProvider` as `false`; vscode-css-language-server from vscode-langservers-extracted —
  it asks `workspace/configuration` for the section `css` before it validates and publishes nothing
  when the answer is `null`, and the two expected warnings when the answer is `{"validate": true}`;
  rust-analyzer 1.99.1 — after `quiescent` it starts `cargo check` as a progress
  under the token `rust-analyzer/flycheck/0` and publishes the compiler's diagnostics for a file
  while that runs, after the diagnostics of its own analysis), GEN (a language feature that
  cannot answer says why); v17: OBS-0039 (side by side 2026-11-13 on a folder with `index.html`, which links
  `css/site.css`, has a jump link to `#prices` and elements with classes and ids: Go to Declaration on a class
  inside a `class` attribute opens the stylesheet at the rule of that class, W1; on the fragment of a jump link it
  puts the caret on the `id` attribute of that name in the same file, W2; the structure lists the elements nested
  as in the document, each named with its id and classes, `link` and `img` with their address beside them, W3;
  IDECraft found nothing at the first two places and listed the elements flat and without id and classes), STD
  (HTML: the `class` attribute is a list of words separated by white space, `id` names one element, the fragment
  of an `href` names an id, `<link rel="stylesheet" href>` and `<style>` bring stylesheets; CSS selectors: `.name`
  selects a class, `#name` an id, names are case-sensitive; URLs: scheme, host, query, fragment and percent
  encoding), OSS (the tree-sitter grammars for HTML, CSS, SCSS and Less the workspace already uses for colouring),
  SPEC-0148 (the project's own sources), SPEC-0156 (opening a file without following a link), GEN (a tool that
  reads files named by file content bounds how many and how large they are and never leaves the project)
- **Feature matrix rows:** F-038

## Purpose
Find a working language server for each language, and when there is none, say exactly why and how to get
one — instead of failing with "command not found" in the middle of a feature.

## Behaviour
1. Each language has an ordered list of candidate servers with command line and an install hint. SQL has
   none: IDECraft's own SQL support (SPEC-0014/0015) serves it. [STD]
2. A candidate is available when its program is an executable file on the `PATH`. A rustup proxy (a
   program in the same directory as `rustup` and identical to it) counts only if `rustup which <program>`
   succeeds; otherwise it is reported as "rustup proxy without the component". [STD, improvement IMP-0019]
   2a. **Checked where the server runs (v15):** a server is started in the project folder, and rustup
   selects the toolchain by that folder. So for a server of a project `rustup which <program>` is
   run in the project folder, not in the folder IDECraft was started from: a project whose
   toolchain lacks the component is told so before the server is started ("… is a rustup proxy,
   and the toolchain rustup selects in <project folder> has no rust-analyzer component", with the
   install hint), instead of the server's last words after it stopped; and a project whose
   toolchain has it is served although the default toolchain does not. Where no project folder
   is known (the overview of behaviour 5) the check runs as before. Nothing else about the lookup
   changes, and nothing more is run than before: the same `rustup which`, in the folder the
   server itself would run in. [STD]
3. The first available candidate is used. If none is, the error names the language, each candidate with
   the reason it is unusable, and the install hints. [GEN, improvement IMP-0019]
4. All LSP features (diagnostics, code insight, rename, code actions) start servers through the registry.
5. MCP tool `language_servers` lists every language with its candidates, which one would be used, the
   problem of unusable ones and the install hints. [ADR-0008]
6. **Ready (v2):** a candidate marked as reporting its state (rust-analyzer: `experimental/serverStatus`)
   is asked only after it says it has loaded the workspace (`quiescent`), waiting at most the request's
   time limit; notifications that arrive meanwhile (e.g. diagnostics) are kept for the feature that
   needs them. Other servers are asked at once. Without this, rust-analyzer answers the first requests
   empty. [STD]
7. **TypeScript fallback (v3):** typescript-language-server gets the `typescript/lib` directory of the
   `node_modules` it is installed in as `tsserver.fallbackPath`, so a project without its own
   `node_modules/typescript` works. The server needs TypeScript 5 (TypeScript 7 ships no `tsserver.js`);
   the install hint says `typescript@5`. [STD]
8. **Checked servers (v3):** pyright, typescript-language-server, gopls and
   vscode-langservers-extracted answer at once after `initialize` (v14: pyright only about the
   opened file, see behaviour 22; v16: intelephense only after its indexing, behaviour 25, and
   clangd after it has parsed the file, behaviour 24). rust-analyzer and jdtls need the wait
   of behaviour 6; jdtls reports readiness with `language/status` of type `ServiceReady`.
9. **Diagnoses first (v3):** jdtls answers about an opened file only after it has published that file's
   diagnostics; such a candidate is asked after them (at most the request's time limit), and the
   diagnostics stay for the feature that needs them. jdtls gets `java.signatureHelp.enabled`, which it
   leaves off by default. The free intelephense has no type declaration, call hierarchy or rename (paid features
   of that server). [STD]

10. **More languages (v4):** XML `lemminx`; YAML `yaml-language-server --stdio`; Markdown `marksman
    server`; TOML `taplo lsp stdio` (the full build; the npm package has no LSP); Shell
    `bash-language-server start`; Dockerfile `docker-langserver --stdio`; Groovy
    `groovy-language-server` (built from source); Scala `metals`; SCSS and Less the CSS server; Swift
    `sourcekit-lsp`; Lua `lua-language-server`; R `R --no-echo -e languageserver::run()`. [STD]
11. **Quirks (v4):** bash-language-server and the R server analyse the opened text only after reading
    their configuration, so they are asked after its first diagnostics (behaviour 9).
    lua-language-server answers "Workspace loading" for a few seconds with no notification when
    done; it is probed with a hover until that text is gone (at most the time limit).
    groovy-language-server ignores the text of `didOpen` and compiles the file on disk; it gets the
    text again as `didChange`. It does not resolve unqualified static calls (server limit). The CSS
    server offers no completion of SCSS/Less variables (server limit). [STD]

12. **Ruby (v6):** Ruby files resolve their server in the workspace (SPEC-0185 behaviours 7–8): the
    candidates are looked up on the selected Ruby's search path, a candidate locked in `Gemfile.lock`
    starts through `bundle exec`, and the server process gets that search path as `PATH`. A resolved
    server therefore carries a command, arguments before and after the candidate's own, and an
    optional `PATH` for its process. [STD]
13. **ruby-lsp readiness (v6):** ruby-lsp indexes the workspace and the bundle's gems after
    `initialize` and answers hover and definition empty until then; it reports this as work-done
    progress with the token `indexing-progress`. It is a server that reports its state
    (behaviour 6), and the `end` of that progress means ready. Its index is built from the files on
    disk, not from opened or changed text, so hover and completion of constants see the saved state
    (server limit). Solargraph answers at once and works on the unsaved text. [STD]

14. **Single Java files (v7):** a `.java` file in a project without `pom.xml` or Gradle files, also
    in a subfolder of a project that is otherwise C++, JavaScript or anything else, is served by
    jdtls started in the project folder: jdtls makes the folder an invisible project with the
    file's folder as source folder and the JDK it runs on, so declarations, usages, completion and
    diagnostics work in that file and between Java files of that folder. Nothing is written into
    the project. Classes of the JDK are found, but their declarations are not files (behaviour 16).
    [STD]
15. **Missing server in the status bar (v7):** when a file opens whose language has candidates but
    none is usable, the status bar says so once per language and session, naming the language, the
    program looked for and its install hint, e.g. "No Java language server: jdtls not found —
    install Eclipse JDT Language Server (`brew install jdtls` …)". Features that need the server
    (Go to Declaration, Find Usages, refactorings, …) say the same instead of a generic "no
    language server answered", and a server that fails to start says why (e.g. jdtls without the
    JDK it needs). [GEN, IMP-0019]
16. **Declarations outside files (v7):** a declaration the server reports at a non-file address
    (jdtls: `jdt://` for classes of the JDK and of libraries) is not dropped silently: when it is
    the only one, the status bar says that the declaration is in a library class and names it, and
    that IDECraft cannot open library sources yet. [STD]
17. **Source roots without a build file (v8):** when the project folder has no `pom.xml`,
    `build.gradle`, `build.gradle.kts`, `settings.gradle` or `settings.gradle.kts`, IDECraft infers
    the Java source roots before it starts jdtls and gives them to it as `java.project.sourcePaths`
    (relative to the project folder):
    - the convention folders `src/main/java` and `src/test/java`, those that exist;
    - for every other `.java` file (not below a root already chosen): the folder from which the
      file's folders equal its declared package (`package lab.inventory;` in `x/lab/inventory/`
      gives `x`); a file without a package gives its own folder; a file whose folders do not end in
      its package gives no root (jdtls reports the mismatch, as it should);
    - a root inside or around a root already chosen is dropped (convention folders first, then
      shorter paths first); the project folder itself is never a root of its own — when it is the
      only one, nothing is given and jdtls behaves as in behaviour 14.

    The search skips hidden folders and `target`, `build`, `out`, `bin` and `node_modules`, and
    stops after 5,000 `.java` files and 20 folder levels. The roots are inferred when the server
    starts (a restart picks up new folders). Nothing is written into the project: no `.project`,
    `.classpath` or settings file. With a build file nothing changes: jdtls imports the build. In
    safe mode the same roots are given (reading files runs no project code). [STD, GEN]
18. **Workspace data of jdtls (v9):** jdtls keeps what it imported for a project in its workspace
    data folder and reuses it on the next start, so roots given later would not reach a project it
    already knows. IDECraft therefore starts every jdtls with its own data folder for the project:
    `jdtls/<hash of the canonical project path>` in IDECraft's user cache folder
    (`$XDG_CACHE_HOME/idecraft`, else `~/Library/Caches/idecraft` on macOS, else `~/.cache/idecraft`;
    the system's temporary folder when there is no home or the project itself lies there). The
    folder records the source roots it was created with (behaviour 17; none for a project with a build file). When the roots inferred
    at the start differ from the recorded ones, or nothing is recorded, the folder is emptied before
    jdtls starts, so jdtls imports the project afresh with the current roots. Equal roots keep the
    folder, so a restart stays fast. Two projects with the same folder name no longer share data.
    Only the cache folder is touched, never the project. [STD]
19. **TypeScript without a syntax-only server (v10):** typescript-language-server is started with
    `tsserver.useSyntaxServer: "never"`. With its default a syntax-only process answers while the project
    loads, so the first search for references after opening a file finds nothing in other files
    (OBS-0028 item 5). [OBS, STD]

20. **Files changed on disk reach the servers (v11):** a file below the project folder that is
    created, changed or deleted by something other than an IDECraft editor buffer (a checkout, a
    generator, another editor, a terminal command) is told to the running language servers of that
    project with `workspace/didChangeWatchedFiles`, change type 1 (created), 2 (changed) or 3
    (deleted). A server that indexes the disk (jdtls) then knows a new class without the file being
    opened, so hierarchies, usages and imports see it (OBS-0027 L35). [OBS, STD]
    - **Source of the changes:** the snapshot comparison of SPEC-0026; no second watcher runs on
      the same folder. In the window the comparison is part of the existing two-second round that
      already looks at the open editors and the build files, and it only runs while a language
      server of the project is running or starting — a project without servers costs nothing. On
      the host of a remote project the backend's own watcher (SPEC-0026 behaviour 3) hands its
      batches on. The first look after a server appears is the baseline and reports nothing: the
      server reads the disk itself when it starts. A rename is told as deleted plus created. [GEN]
    - **Which server gets which file (static interest):** IDECraft does not honour watchers that
      servers register dynamically and therefore does not announce
      `workspace.didChangeWatchedFiles.dynamicRegistration`; `client/registerCapability` stays
      answered without effect. Instead a server gets the files of the languages it serves
      (SPEC-0042 behaviour 1: the file's language has the server's program among its candidates)
      and the build files of its ecosystem: `pom.xml`, `build.gradle`, `build.gradle.kts`,
      `settings.gradle`, `settings.gradle.kts` and `gradle.properties` for the Java, Kotlin, Groovy
      and Scala servers; `package.json`, `tsconfig.json` and `jsconfig.json` for the TypeScript,
      JavaScript, Vue and Svelte servers; `Cargo.toml` and `Cargo.lock` for the Rust server;
      `pyproject.toml`, `requirements.txt` and `setup.cfg` for the Python servers; `go.mod`,
      `go.sum` and `go.work` for the Go server. Other files are told to nobody. Only servers whose
      workspace folder contains the file get it. [STD, GEN]
    - **Folders that are never told:** version-control and IDE folders (`.git`, `.hg`, `.svn`,
      `.idea`, `.idecraft`), build output and caches (`target`, `node_modules`, `__pycache__`,
      `.gradle`, and `build` and `out` unless a folder named `src` lies above them — a package
      may be called `build`), and virtual environments (`.venv`, `venv`, and any folder that holds
      a `pyvenv.cfg`). The window's look at the disk does not even descend into them. [GEN]
    - **Security:** only paths inside the project folder are told. A path with `..`, a path
      outside the folder, a symbolic link, and a path that reaches its file through a linked
      folder leading out of the project are dropped; links are never followed. The URI is built
      from the workspace folder the server was started with. [GEN]
    - **Coalescing and debounce:** changes are collected per project and sent once nothing new
      came for 400 ms, at the latest 2 s after the first collected change. Per path only the net
      effect is sent: created then changed is created; created then deleted is nothing; changed
      then deleted is deleted; deleted then created is changed; repeated changes are one. All
      changes of one flush go to a server in one notification. [GEN]
    - **Cap:** one notification carries at most 500 changes; more are sent as several
      notifications in path order with 50 ms between them, so requests of the user get in between.
      Nothing is dropped below 10,000 changes, because a server that missed a change would stay
      wrong. When one flush holds more than 10,000 changes for a server (a checkout across a large
      tree), that server gets none of them and is stopped instead; the next question starts it
      again and it reads the disk afresh (as SPEC-0278 behaviour 7 does after a trust change).
      [GEN]
    - **Own saves:** a save from an IDECraft editor is also a change on disk and is told as
      changed, in addition to the document notifications. For an open document the server keeps
      the editor's text as the truth (LSP), so this is harmless; servers that index the disk need
      it for files saved by a refactoring that are not open. [STD]

21. **TypeScript 7 (v12):** TypeScript 7 is a native program without `lib/tsserver.js`, so
    typescript-language-server cannot run on it; TypeScript 7 brings its own language server
    instead (`tsc --lsp --stdio`, reported as `typescript-go`). Which TypeScript a program comes with
    is read from the `typescript` package in the `node_modules` that holds the program (links
    followed; its `package.json` version, `lib/tsserver.js`, `lib/getExePath.js`).
    - typescript-language-server with TypeScript 7 beside it is not usable; the reason names the
      version and the fix (`npm install -g typescript@5` next to it). Before v12 it was started,
      failed at `initialize` and the language stayed without a server.
    - A third candidate after vtsls is `tsc --lsp --stdio`: usable only when its package is
      TypeScript 7 or later. IDECraft starts the package's native program for this system
      (`@typescript/typescript-<os>-<arch>/lib/tsc`, inside the package or hoisted beside it)
      directly. Without that program the reason names the package and the fix
      (`npm install -g typescript@7`); a `tsc` of TypeScript 5 or older is said to have no language
      server of its own.
    - This is the TypeScript installed with the program, never the project's, so it also starts
      in safe mode (SPEC-0278 behaviour 9).
    - The language-server overview (`language_servers`) and the status bar's missing-server
      message (behaviour 15) give these reasons. [STD, GEN]
    - **(v13)** TypeScript 7's server does not publish a file's diagnostics; it gives them when
      asked (LSP 3.17 `textDocument/diagnostic`, its `diagnosticProvider`). IDECraft asks for them
      as it does for vue-language-server (SPEC-0141 v4 behaviour 9), so errors reach the editor and
      `file_diagnostics`. Before v13 a file checked by it showed no problems at all. [STD]

22. **Project files listed first (v14):** pyright and basedpyright list the project's source files
    only after they have started and read their settings. A question that spans files
    (`textDocument/references`, `textDocument/rename`) that arrives before the listing is answered
    without an error from the opened file alone, so the answer looks complete and is not. The
    servers publish the diagnostics of the first opened file after the listing. A candidate marked
    as listing its files first is therefore asked only after those diagnostics have arrived (at most
    the request's time limit; the diagnostics stay for the feature that needs them):
    - a server started for one question (`idecraft call`, MCP without a running window): after the
      diagnostics of the opened file;
    - a server of the pool (SPEC-0090): after the diagnostics of the first file opened in it; later
      files are asked at once, because the listing is done.
    - When the diagnostics do not come within the time limit, the question is asked anyway, as
      before v14; Find Usages keeps its own deadline and text fallback (SPEC-0293 behaviours 2
      and 10).
    Before v14 the first question to a fresh server lost the race: Find Usages on a class of
    `app/prices.py` and `find_symbol_locations` with `references` gave the usages of that file
    only. The rename plan was complete only because its `textDocument/prepareRename` question went
    first and the listing finished meanwhile; it is no longer left to that. Other servers are
    unchanged (pylsp included). [OSS, STD, OBS]

23. **rustup installs nothing and names the toolchain (v16):** the check of behaviour 2a runs
    `rustup which` with `RUSTUP_AUTO_INSTALL=0`. Without it rustup downloads the toolchain a
    project's `rust-toolchain.toml` names when that toolchain is missing; with it the check fails
    instead, and because a server whose check fails is not started, the proxy does not download
    one either: a project file never makes IDECraft fetch a toolchain, in safe mode or not. When
    rustup's answer names the toolchain it chose for the folder, the message names it with the
    folder and the command for exactly that toolchain ("… is a rustup proxy without the component
    in the toolchain <name> chosen for <project folder> (rustup component add rust-analyzer
    --toolchain <name>)"), or says that the toolchain is not installed and names `rustup toolchain
    install <name> --component rust-analyzer`; otherwise the message of behaviour 2a stands. The
    MCP tool `language_servers` and the host's `lsp.status` answer for their workspace folder
    instead of the folder IDECraft runs in. [STD, OSS, GEN]
24. **clangd after it has parsed the file (v16):** clangd is a candidate that is asked about an
    opened file only after it has published that file's diagnostics (behaviour 9): until it has
    built the file's syntax tree it completes from the words in the file, so `shape.` offered
    `print`, `const` and `include` instead of the members of the class. [OSS]
25. **intelephense after its indexing (v16):** intelephense indexes the workspace and its PHP
    stubs after `initialize` and reports it as a work-done progress whose title names
    "Intelephense" and "indexing", under a token it makes up. It is a server that reports its
    state (behaviour 6); the `end` of that progress means ready. The diagnostics it published for
    a file before that are about a project it had not read; they are dropped, and the ones it
    publishes after the indexing are the answer. Before v16 every question to a freshly started
    intelephense — hover, completion, declarations, usages, signature and the diagnostics — came
    back empty without a message. The free edition does not rename; see SPEC-0023 behaviour 11.
    [OSS, STD]
    - **(v19) Without a progress:** intelephense up to 1.12 reports its indexing only with its own
      notifications `indexingStarted` and `indexingEnded`; `indexingEnded` means ready as the end of
      the progress does, with the same dropping of the diagnostics from before. Before v19 every
      question to such an intelephense waited the whole wait (120 s) for a progress that never
      came. [OBS: LSP trace of intelephense 1.12.6 on the build server, 2026-11-13]
    - **(v19) With its stored index:** intelephense keeps its index between starts and indexes only
      what changed; when nothing did, it begins no indexing and says nothing more after its search
      for files, in 1.12 and 1.18 alike. A server marked so is also ready when it has begun no
      indexing (neither `indexingStarted` nor its indexing progress) and has sent no message for
      3 s; every message starts the 3 s again, and an indexing once begun is waited for to its
      end. This holds for the server asked once and for the pool's running server, which waits
      before the first file is opened. Before v19 the second start of intelephense for a project
      waited the whole wait for each question. [OBS: same trace, second start; decision of this
      version]
26. **Validation settings of the CSS server (v16):** vscode-css-language-server asks for its
    settings before it checks a file and publishes nothing when it gets none. Its question for the
    sections `css`, `scss` and `less` is answered with `validate: true`, so unknown properties,
    empty rules and the other problems it knows are reported for `.css`, `.scss` and `.less`
    files, with its quick fixes. Before v16 these files never showed a problem. [OSS, STD]
27. **rust-analyzer's diagnostics after its check (v16):** rust-analyzer publishes what the
    compiler says about a file only when `cargo check`, which it starts by itself after loading
    the workspace, has run; the diagnostics it publishes first come from its own analysis and are
    often empty. A question for a file's diagnostics (MCP `file_diagnostics`, quick fixes) to a
    pooled rust-analyzer therefore answers with the last diagnostics published once that check has
    ended — a progress whose token starts with `rust-analyzer/flycheck` — waiting at most the
    request's time limit. When no check has begun within 3 seconds after the server said it was
    ready (the project is in safe mode, where the check is switched off, or the check is turned
    off in the project), the diagnostics published so far are the answer. The editor is unchanged:
    it shows every publication as it comes. [OSS, STD]
28. **A class or an id leads to its rules (v17, OBS-0039 W1):** in an HTML file (`.html`, `.htm`),
    Go to Declaration (SPEC-0071) and the MCP tool `find_symbol_locations` with kind `definition`
    answer from IDECraft's own syntax trees when the caret stands in, directly before or directly
    after a word of a `class` attribute's value, or the value of an `id` attribute: the targets are
    all class selectors `.name` of that word, for an id all id selectors `#name`. Each target
    reaches from the `.` or `#` to the end of the name. They are looked for, and listed, in this
    order:
    a. the stylesheets the file links: every `<link>` whose `rel` holds the word `stylesheet`
       (in any case), in the order of the links, when its `href` names a CSS, SCSS or Less file
       among the project's own sources. The address counts from the folder of the HTML file, one
       that starts with `/` from the project folder; a query and a fragment are cut off and
       percent sequences are decoded. An address with a scheme (`http:`, `https:`, `data:`,
       `file:` and every other), one that starts with `//`, one with a backslash, and one that
       leaves the project folder name no file: nothing is ever fetched from a network and nothing
       outside the project is read;
    b. the `<style>` blocks of the file itself, in the editor's text (CSS; SCSS or Less when the
       block says so with `lang`);
    c. the other CSS, SCSS and Less files among the project's own sources (SPEC-0148 v3
       behaviour 7), ordered by path.
    Several rules give several targets (the chooser of SPEC-0071 v3). A name is compared with a
    selector exactly and case-sensitively, character by character; no search pattern is built from
    file content. A selector that escapes a character with a backslash (`.a\:b`) is compared
    without the backslash. A name no stylesheet defines has no target, and the editor says so as
    for any symbol without a declaration. [OBS-0039, STD, OSS, GEN]
29. **A jump link leads to the id (v17, OBS-0039 W2):** in the value of an `href` attribute of an
    HTML file, with the caret on the `#` or behind it, the targets are the elements whose `id`
    equals the fragment (percent sequences decoded; exactly and case-sensitively): in the same
    file, read from the editor's text, when nothing stands before the `#`; in the file named
    before the `#`, under the rules for addresses of behaviour 28a, when that is an HTML file
    among the project's own sources. The target is the value of the `id` attribute. With the
    caret before the `#` the language server is asked as before. [OBS-0039, STD]
30. **A selector lists its uses in HTML (v17):** Find Usages (SPEC-0293) and the MCP tool
    `find_usages` on a class selector of a CSS, SCSS or Less file keep what the stylesheet's
    language server answers and add every word of a `class` attribute that equals the name in
    the HTML files among the project's own sources; on an id selector, every `id` value. A place
    is listed once. The added usages are marked as found by IDECraft's own index: the search
    carries a note that says how many of the usages come from it, and each of them has
    `source: "own_index"` in the answer of `find_usages`. When the server cannot be asked at all,
    the search fails as before; when it is too slow, the added usages stand beside the text
    occurrences. [GEN, STD]
31. **The structure of an HTML file is nested (v17, OBS-0039 W3):** the symbols of an HTML file
    (the File Structure popup and the Structure tool window of SPEC-0079, the MCP tool
    `document_symbols`, and everything else that asks for a file's symbols) come from IDECraft's
    own syntax tree of the text, saved or not, and no language server is asked: one symbol of kind
    `element` per element, in the order of the document and nested like it (`head` below `html`
    with its children, `body` with its children and theirs). An element is named by its tag as
    written, then `#` and its id when it has one, then `.` and the word for every word of its
    `class` (`main#prices.card`, `button.button.primary`). A `link` and an `a` carry the value of
    `href` as detail, an `img` and a `script` the value of `src`, white space folded and cut after
    200 characters. Text, comments and the document type are not listed; `script` and `style` are
    listed without their content. A symbol starts at the tag name and ends where the element
    ends. At most 20 000 elements of one file are listed. Markup with errors lists what the
    parser still reads as elements. Breadcrumbs and sticky lines (from the syntax tree's
    declarations) and Go to Symbol (from the search index) do not read these symbols and stay as
    they are. [OBS-0039, OSS]
32. **Bounds and safe reading (v17):** for one question of behaviours 28 to 30 IDECraft reads at
    most 200 stylesheets (the linked ones first, then the others by path) and at most 500 HTML
    files, each of at most 1 MiB; a larger file and the files beyond the bound are left out
    without a message. Only the project's own sources are read (SPEC-0148): nothing in installed
    dependencies such as `node_modules`, in ignored or excluded folders, behind a symbolic link or
    outside the project folder. A file is opened through a handle that does not follow a link at
    its last name, and its size is taken from that handle (SPEC-0156 v4 behaviour 14). The names
    found in a file are remembered in memory with the file's modification time and size and read
    again when either differs; at most 2048 files are remembered, then the memory starts empty.
    Other files count with their saved content. [GEN]
33. **No server needed (v17):** behaviours 28, 29 and 31 answer without the HTML language server,
    also when none is installed. At every other place of an HTML file, and for every other
    question, the server is asked as before. [GEN]
34. **Opened text after a background read (v18):** bash-language-server (5.4 to 5.8) reads the
    project's saved shell files in the background once it has its settings, and its reading of a
    file replaces that of the opened text: a question about a file whose text in the editor differs
    from the saved file was answered from the saved content (no symbols, definitions or completions
    of the unsaved part). It publishes the opened file's first diagnostics only after that read. A
    server marked as reading the saved files therefore gets the first file opened in it again
    (`didChange` with the whole text) after those diagnostics when the text differs from the saved
    file, and the question waits for the diagnostics of that text and answers with the newest list;
    in the pool of running servers this happens once per server. [OBS: live test with
    bash-language-server 5.4.3 on the server CI, 2026-11-13; STD]
35. **Kotlin answers after compiling (v20):** kotlin-language-server (1.3.13) reports neither
    progress nor readiness. Asked right after a file was opened, it answers from the file's syntax
    alone while it still resolves the classpath and compiles: semantic tokens of the declarations
    without the references (7 instead of 41 tokens, a `$name` in a string template missing), no
    references from other files, no parameter hints. It publishes the file's diagnostics once it has
    compiled it — an empty list for a file without problems. A question to it therefore waits for
    those diagnostics first, for a server of its own and in the pool of running servers after each
    change of the text, as for servers that diagnose first; editor features asked through the pool
    (highlighting, usage counts, hints) get the full answer instead of a partial one that stays
    until the next change. [OBS: live test `kotlin_semantic_tokens_with_the_real_server` on the
    server CI under load, 2026-11-13, LSP trace; STD]
36. **intelephense's second list (v20):** intelephense counts as ready when it stays silent for
    3 s without beginning an indexing (behaviour 25). It publishes the diagnostics of an opened file
    twice: with its stored index first a quick list and then the list checked against the index; when
    it indexes, an empty list (before or during the indexing) and the real one after
    `indexingEnded`. Under load the quick list was empty while the checked one named the problem
    (the diagnostics test got `[]`: server CI, 2026-11-13; LSP traces on the build server with intelephense
    held to one busy core: `[] L` in 3 of 18 starts, `L L` with an unchanged index, `[] indexing L`
    when indexing). A question for its diagnostics therefore takes the second list of the opened
    file, or the first when no second comes within 30 s; what it publishes between
    `indexingStarted` and `indexingEnded` is no answer and counts for nothing. This holds for a
    server of its own and in the pool (where a file opened again counts anew). [OBS: LSP traces of
    intelephense 1.12.6 on the build server, 2026-11-13; STD]

37. **The wait asked for is the question's wait (v21):** definitions, references, type definitions
    and implementations of a position are asked with the wait their caller gives (Go to
    Declaration and the other navigation of the window 30 s, SPEC-0071; Quick Definition and Change
    Signature 30 s; a remote project's host 30 s; the agent tools 60 s); a question that runs out
    of time is withdrawn. Until v20 that wait bounded only the opening of the file, and the
    question itself had the client's fixed 60 s whatever was asked, so a caller that asked for
    more (a test giving 120 s) still got 60 s, and the window waited up to 90 s instead of its 30 s.
    A server that builds its search index on the first such question (jdtls) may answer later than
    the wait under load; the pooled server keeps building it, so a question asked again is
    answered. [GEN]
## Acceptance criteria
- **AK-1** With a crafted `PATH`: the first available candidate wins over later ones; a missing program is
  skipped; a non-executable file is skipped; a rustup proxy whose `rustup which` fails is reported as such,
  one whose `rustup which` succeeds is used.
- **AK-2** The error for a language without a usable server lists candidates, reasons and install hints;
  SQL reports that IDECraft serves it itself.
- **AK-3** Existing LSP features still work with `clangd` through the registry (existing tests).
- **AK-4** The MCP tool lists languages and candidates with availability.
- **AK-5** With rust-analyzer on a Cargo project and unsaved text: hover, definition, references, type
  declaration, implementation, signature help, symbols, call hierarchy, completion, rename, formatting
  and code actions answer (`tests/rust_analyzer.rs`).
- **AK-6** With pyright (Python), typescript-language-server (TypeScript, project without
  `node_modules`), gopls (Go), intelephense (PHP) and jdtls (Java) on unsaved text: hover with the doc comment,
  definition, references, type declaration, signature help with the active parameter, symbols, call
  hierarchy, completion and rename answer, as far as the server offers them; CSS hover, symbols and
  completion, HTML completion and JSON symbols answer (`tests/servers.rs`).
- **AK-7** For each language of behaviour 10 the outline answers, and hover, definition and completion
  where the server offers them, on unsaved text; Lua answers within seconds, not after the time limit
  (`tests/servers.rs`, 18 tests). CI installs every server except Swift's toolchain.
- **AK-8 (v5)** With kotlin-language-server: outline, hover and definition on a Kotlin file
  (`tests/servers.rs`: `kotlin_language_server`, optional in CI).

- **AK-9 (v6)** Ruby resolution in a workspace as SPEC-0185 AK-5; the other languages resolve as before
  (existing tests). With ruby-lsp, hover answers on the first request after start (SPEC-0185 AK-7).
- **AK-10 (v7)** With jdtls, a project folder holding C++ and npm files and `sprachen/test.java`
  (two classes, no build file): Go to Declaration finds a constructor, a local variable and a
  method, directly and through the pool (`tests/servers.rs`:
  `jdtls_standalone_file_in_subfolder`; `tests/pool_java.rs`); `String` is reported as the library
  class `java.lang.String` (`tests/pool_java.rs`; names: `insight::tests::places_outside_files_are_named`).
- **AK-11 (v7)** With a crafted `PATH` without jdtls, the status message names Java, `jdtls` and
  the install hint; with jdtls, and for SQL, it is none (`servers::tests::missing_server_message`).
- **AK-12 (v7)** By hand (round 5, Nr. 12, 16 and 209): ⌘B in `sprachen/test.java` of the hand-test
  project jumps; without jdtls on the `PATH` the status bar names it.
- **AK-13 (v8)** Inference (unit tests): a Maven layout gives `src/main/java` and `src/test/java`;
  a file in `x/lab/inventory/` with `package lab.inventory;` gives `x`; a mismatching package gives
  no root; nested roots are dropped; a build file or only top-level files give none; hidden and
  build folders are skipped. With jdtls, a copy of `tests/reference-projects/java-basic` without a
  build file: `src/main/java/lab/inventory/Inventory.java` has no package diagnostic and the project
  folder lists the same files before and after (`tests/java_source_roots.rs`, skipped without jdtls).
- **AK-14 (v9)** Workspace data (unit tests): the folder of a project lies in the cache folder,
  differs for two projects with the same name and never lies inside the project; it is kept with
  the same roots and emptied when the roots change or none are recorded; the jdtls command line
  carries `-data` with it, other servers' do not. With jdtls, a copy of `java-basic` whose data
  folder was first filled by a jdtls without source roots (as before v8): after the next start
  through IDECraft `Inventory.java` has no package diagnostic (`tests/java_source_roots.rs`,
  skipped without jdtls).
- **AK-15 (v10)** The first Find Usages after opening a `.ts` file lists the use in another file
  (`crates/languages/tests/it/servers.rs`: `typescript_inlay_hints`, skipped without the server).
- **AK-16 (v11)** Unit tests: per path only the net effect remains (created then deleted nothing,
  created then changed created, changed then deleted deleted, deleted then created changed); the
  flush is due 400 ms after the last change and at the latest 2 s after the first; the excluded
  folders are dropped and `src/…/build/` is kept; a path with `..`, outside the project, behind a
  linked folder or a link itself is dropped; jdtls gets `.java` files and `pom.xml` but no `.py`
  file, rust-analyzer gets `Cargo.toml`; the notification carries `file://` URIs with the types
  1, 2 and 3, and 1,201 changes are three notifications of at most 500; two looks at a folder
  report what was created, changed, deleted and renamed between them and nothing from `build`
  or a virtual environment.
- **AK-17 (v11)** With jdtls on a project with `Animal` and one subclass: a second subclass file is
  written to disk by the test without opening it; after the next look at the disk the class
  hierarchy of `Animal` lists both subclasses, and after deleting the file only the first
  (`crates/languages/tests/java_watched_files.rs`, skipped without jdtls).
- **AK-18 (v11)** The snapshot of a workspace skips the folders the caller excludes in addition to
  the standard ones (unit test in `idecraft-core`).

- **AK-19 (v12)** `servers::tests::typescript_7_starts_its_own_server` (npm layouts as `npm install -g`
  makes them: TypeScript 7 with its native program gives `tsc --lsp --stdio` with that program, also in
  safe mode, and typescript-language-server is unusable with the version and the fix; without the
  native program the error names its package and `npm install -g typescript@7`; TypeScript 5 keeps
  typescript-language-server and its `tsc` is no server).
- **AK-20 (v12)** With TypeScript 7.0.2 installed (`crates/languages/tests/it/servers.rs::typescript_7_brings_its_own_server`):
  IDECraft resolves `tsc --lsp --stdio` and Find Usages through it finds the use in another file.
  Tests that need typescript-language-server's behaviour run only when IDECraft picks it.

- **AK-21 (v13)** With TypeScript 7.0.2 installed: `file_diagnostics` on `const n: number = "x";` gives an
  error at line 1 (`tests/functional/f036_javascript.rs::typescript_errors_come_from_the_language_server`);
  `servers::tests::only_vue_and_typescript_7_pull_diagnostics`.

- **AK-22 (v14)** With pyright or basedpyright installed, in a project with a class, a method and a
  function in `app/prices.py` used from `main.py` and `test_prices.py`, each asked of a freshly
  started server (`crates/languages/tests/it/servers.rs::pyright_usages_in_other_files_on_the_first_question`):
  Find Usages on the class gives 1, 3 and 3 usages in the three files, on the function 0, 2 and 2,
  on the method 1, 0 and 1, all from the language server; the references of the class name both
  other files; the rename plan of the class has 2, 3 and 3 edits. With the pool
  (`tests/py_usages_pool.rs::pooled_pyright_finds_usages_in_other_files_at_once`): the first Find
  Usages after the server's start gives both usages in the other file. Both tests fail without the
  wait. `servers::tests::only_pyright_lists_the_project_files_first`: only the two candidates are
  marked. Skipped without pyright and basedpyright.
- **AK-23 (v15, behaviour 2a)** With a stand-in rustup that has the component only in one of two
  folders: the proxy is available for the project in the first and unusable for the project in
  the second, the reason names that folder and the missing component, and the error of the
  registry carries the install hint (`servers::tests::rustup_proxy_is_checked_in_the_project`).
  By hand: `idecraft call <folder> format_file` on a Rust file of a project whose toolchain has
  rust-analyzer while the default toolchain has not, and the other way round.
- **AK-24 (v16)** `servers::tests::rustup_proxy_is_asked_in_the_project_folder`: with a rustup
  stand-in that has the component only in one folder, the server resolves for that folder (also in
  safe mode) and not for another; the message names the toolchain, the folder and the command with
  `--toolchain`; rustup was never run without `RUSTUP_AUTO_INSTALL=0`. Headless on this Mac
  (default toolchain without the component, project with `rust-toolchain.toml` naming `stable`):
  all Rust tools answer from any working folder; a folder naming a toolchain that is not installed
  gets the message and no download.
- **AK-25 (v16)** With clangd (`crates/languages/tests/it/servers.rs::clangd_completes_members_after_parsing`):
  the completion after `box.` in a freshly opened C++ file lists the field and the method with
  their kinds and not a local variable of the function. Skipped without clangd.
- **AK-26 (v16)** `lsp::tests::indexing_progress_of_intelephense_and_ruby_lsp` (the titled progress
  under any token, ruby-lsp's token, not rust-analyzer's indexing) and
  `lsp::tests::diagnostics_from_before_the_index_are_dropped`. With intelephense
  (`servers.rs::intelephense_is_asked_after_its_indexing`): in a project of two files the hover on
  a method declared in the other file has its signature, the diagnostics name the undefined
  method, and renaming says that the server does not offer it. Skipped without intelephense.
- **AK-27 (v16)** `servers::tests::readiness_and_settings_of_clangd_intelephense_css_and_gopls`;
  with the CSS server (`servers.rs::css_server_reports_problems`): a `.css` and a `.scss` file
  with the property `colr` each get a diagnostic naming it. Skipped without
  vscode-langservers-extracted.
- **AK-28 (v16)** `pool::tests::background_checks_settle`: not settled before the server is ready,
  while a check may still begin and while one runs; settled when it has ended or none began in
  time; other progress does not count. Headless with rust-analyzer: `file_diagnostics` on a file
  with `let count: u32 = "three";` gives the compiler's E0308 where it gave an empty list.
- **AK-29 (v17, behaviour 28; OBS-0039 W1)** In a folder with `index.html` that links
  `css/site.css` (`html_css::tests::class_and_id_lead_to_their_rules`): the caret in `card` of
  `class="card"`, also directly before and after the word, gives the rules `.card` and
  `.card:hover` of `css/site.css`; the second word of `class="button primary"` its rule; an `id`
  value its `#` rule; a tag name, the address of the stylesheet and the attribute name give no
  answer of IDECraft's own. `several_definitions_linked_first`: the linked stylesheet (written as
  `./css/../css/site.css?v=3#top` in a `LINK` with `REL="alternate StyleSheet"`) comes first, then
  the `<style>` block, then an SCSS and a Less file by path; `.Card` and `.card-wide` do not
  count; a stylesheet in `node_modules`, in an ignored folder, outside the project (`../`) and
  behind `https:` or `//` is not read. `addresses_and_names`: which addresses name a file.
- **AK-30 (v17, behaviour 29; OBS-0039 W2)** `html_css::tests::jump_links_lead_to_the_id`: the
  fragment of `href="#prices"` gives the value of `id="prices"` in the same file;
  `docs/other.html#top` the element with `id="top"` of that file and not the one with `id="Top"`
  or `class="top"`; the path before the `#` and an `href` without fragment give no answer of
  IDECraft's own; a fragment no element has, an address with `https:` and one outside the project
  give no target.
- **AK-31 (v17, behaviour 31; OBS-0039 W3)** `html_css::tests::structure_is_nested_and_named`: the
  thirteen elements of the sample with their depths, `header#masthead.banner`, `main#prices.card`,
  `button.button.primary`, the details of `link`, `img`, `a` and `script`, no row for text, the
  comment or the document type; `structure_of_broken_and_huge_documents`: unclosed elements, the
  bound of 20 000 and 3000 levels of nesting. `structure::tests::elements_keep_document_order`:
  the File Structure order leaves elements in document order.
- **AK-32 (v17, behaviour 30)** `html_css::tests::selector_lists_its_uses_in_html`: `.card` lists
  the `class` words `card` of two HTML files, not `Card`, `card-wide`, an `id="card"` or a file in
  `node_modules`; `#prices` lists the `id`; a property and a tag selector give none. With
  vscode-css-language-server (`crates/languages/tests/it/html_css.rs::css_selector_usages_include_html`):
  Find Usages on `.card` of `css/site.css` lists the use in `index.html`, marked as from the own
  index, with a note. Skipped without vscode-langservers-extracted.
- **AK-33 (v17, behaviours 32 and 33)** `html_css::tests::bounds_links_and_changes`: a stylesheet
  changed on disk is read again, one larger than 1 MiB and one that is a symbolic link are not
  read. `crates/languages/tests/it/html_css.rs::html_answers_without_a_server`: `found_in` and
  `symbols_in` answer for the sample on unsaved text with no time given to a server (a wait of
  zero); MCP (`crates/mcp/tests/it/protocol.rs::html_navigation_and_structure`):
  `find_symbol_locations` and `document_symbols` on the sample.

- **AK-34 (v18, behaviour 34)** With bash-language-server installed
  (`crates/languages/tests/it/servers.rs::bash_language_server_shell`, the file on disk a stub, the
  text only in the editor): the document symbols `greet` and `NAME`, definition and completion come
  from the editor's text; before v16 the symbols were empty.
- **AK-35 (v19, behaviour 25)** `lsp::tests::{intelephense_without_progress_ends_its_indexing_by_notification,
  a_server_with_its_stored_index_is_ready_when_quiet}`: `indexingEnded` ends the wait and drops the
  diagnostics from before; a server that read its stored index is ready after the quiet time, and
  one that began indexing (notification or progress) is waited for. With intelephense 1.12.6
  (`crates/languages/tests/it/servers.rs::{intelephense_php, intelephense_is_asked_after_its_indexing}`):
  each question to a server of its own is answered, also from the second start on; before v19
  both tests ran past 300 s.
- **AK-36 (v20, behaviour 35)** With kotlin-language-server
  (`crates/languages/tests/it/kotlin_structure.rs`: `kotlin_semantic_tokens_with_the_real_server`,
  `kotlin_usage_counts_with_the_real_server`, `kotlin_parameter_hints_with_the_real_server`): every
  question is asked once, without repeating it after a pause, and gets the full answer — the
  references among the tokens, the counts with the uses in the other file, the hints — also on a
  machine under load (three runs of the live tests with a build running beside them); before v20
  the tokens test failed about one run in five under load.
- **AK-37 (v20, behaviour 36)** Unit test
  `insight::tests::the_second_list_of_a_server_that_publishes_twice_is_the_answer`: an empty first
  list followed by the checked one gives the checked one; a single list is given after the wait; a
  list during an indexing is none, also when the indexing begins after the silence; a server that
  publishes once is answered by its first list. With intelephense
  (`crates/languages/tests/it/servers.rs`: `intelephense_is_asked_after_its_indexing`): the
  diagnostics name the missing method also with intelephense held to one busy core (6 of 6; before
  v20 one in four failed).
- **AK-38 (v21, behaviour 37)** With jdtls (`crates/languages/tests/java_navigation.rs`:
  `implementations_usages_and_inherited_members`): the implementations are found also under CPU
  stress on all cores, the question asked again after a timeout until the pooled server answers;
  three stress runs green (report `kotlin-live-test-under-load.md`; before v21 one in three failed
  with `Timeout("textDocument/implementation", 60)` although 120 s were asked).
## Out of scope
Installing servers automatically, per-project server settings, servers over TCP, version checks;
showing the sources of library classes (`java/classFileContents`); Java files outside the project
folder; marking source roots by hand (v8: they are inferred only); updating the roots of a running
server when folders are added (v9: the next start renews the data); removing the data folders of
projects that are no longer opened; v11: watchers registered by servers (glob patterns and kinds
of `client/registerCapability`), native file events, folders as such (only files are told), and
holding back the change event of IDECraft's own saves. v17: completion of class names and ids in HTML, renaming a
class or id across HTML and stylesheets, Find Usages started on a class or id in the HTML file, problems for a
class no stylesheet defines, and the same navigation in Vue, Svelte, JSX and template files.

## Known deviations from the reference
The reference bundles its own analysis for many languages; IDECraft uses external servers and reports
missing ones explicitly.

v12: with TypeScript 7 the native server answers with its own defaults: the inlay-hint settings that
typescript-language-server gets (SPEC-0291) are not sent to it, and a project's own TypeScript 5 in
`node_modules` is not used while TypeScript 7 is installed with the server programs. Not compared
next to the reference.

v14: on `~/Projects/cmp-pyx-ide` (OBS-0034 item 38) Find Usages on the class `Item` lists 11 usages
in four files where the reference lists 10; which place the reference leaves out was not read off.
The wait is tied to pyright's order of listing and diagnosing; a future pyright that publishes
diagnostics before it lists the files would bring the incomplete first answer back, and AK-22 would
fail.

v16: limits of the servers themselves, found in the same run and left as they are: the HTML server
knows nothing of the stylesheets a page links (no completion of class names, no declaration of a
class or id in the CSS file, no usages across the two files, no problems of its own for HTML), and
it lists a file's elements without their nesting; the CSS server knows one file at a time;
rust-analyzer offers no type hierarchy; the free intelephense has no rename, implementations, type
declarations, code actions or type hierarchy; gopls gives no parameter information while the
caret stands directly before a string literal. For features the free intelephense marks as
missing other than rename, an empty answer is still returned without a message. Not compared next
to the reference.

v17: the limits of the HTML server named for v16 are closed for the declaration of a class or id,
for jump links and for the nesting of the structure (behaviours 28 to 31); completion of class
names, usages started in the HTML file and problems for HTML stay as v16 says. What stays
different from the reference:
- A class that a script, a template or a framework puts together (`classList.add(…)`, `{{ … }}`,
  a class bound in Vue, Svelte or JSX) is not resolved; only words written in a `class` attribute
  of an HTML file count.
- Which rule the reference opens when several stylesheets define a class was not observed
  (OBS-0039, open question). IDECraft offers all of them, the linked stylesheets first.
- A stylesheet that is reached only through `@import` of a linked one does not count as linked; its
  rules are found among the project's other stylesheets, after the linked ones.
- A selector built by nesting in SCSS or Less (`&-wide` below `.card`) is not found under its
  resulting name, and a selector that escapes a character by its code point is compared as written.
- The reference shows the text of a button beside it in the structure; IDECraft shows no text.
  Elements have one icon for all tags.
- Stylesheets and other HTML files count with their saved content, also while a tab holds unsaved
  changes of them.
- Go to Declaration on an `id` value and Find Usages on a selector (behaviour 30) were not
  compared next to the reference.
