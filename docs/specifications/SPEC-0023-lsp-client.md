# SPEC-0023: Language Server Protocol client

- **Version:** 5 · **Date:** 2026-11-16 (v5: diagnostics of several files in one call, behaviour 13, AK-9, OBS-0071 O1; deviation IMP-0455; v4: `file_diagnostics` filters by severity, behaviour 12, AK-8, OBS-0071 G4; deviation IMP-0441); 2026-11-13 (v3: a request the server does not know is reported as a feature it does
  not offer, behaviour 11; v2: configuration answered from settings, SPEC-0170)
- **Sources:** STD (Language Server Protocol 3.17: base protocol with `Content-Length` framing, JSON-RPC
  2.0 requests, responses, notifications and server-to-client requests; `initialize`/`initialized`,
  `textDocument/didOpen`, `textDocument/publishDiagnostics`, `shutdown`/`exit`; RFC 8089 `file` URIs),
  ADR-0005 (language services via LSP), SPEC-0004 (language identification); v3: STD (JSON-RPC 2.0
  error code `-32601`, method not found; LSP server capabilities such as `renameProvider`, which a
  server may state as `false`), OSS (measured over the protocol: vscode-html-language-server
  answers `textDocument/codeAction` with `-32601`, intelephense 1.18.5
  `textDocument/prepareCallHierarchy`, rust-analyzer `textDocument/prepareTypeHierarchy`;
  intelephense states `renameProvider: false` and answers a rename with nothing), GEN (an error
  message names what is missing in the user's words); v5: STD (one server serves many open documents and
  publishes diagnostics per document URI), OBS-0071 (O1: several files in one call; the reference's
  batch fails as a whole for one missing file and leaves a file it cannot analyse out without a word)
- **Feature matrix rows:** F-032 (diagnostics; base for F-031, F-033, F-034, F-038)

## Purpose
Talk to any language server over stdio, so code insight comes from standard servers, and never hang or
lose the reason when a server misbehaves.

## Behaviour
1. Messages are framed with a `Content-Length` header (other headers ignored, names case-insensitive) and
   a UTF-8 JSON body. [STD]
2. Requests get increasing integer ids; each response is delivered to its request. A response error keeps
   code and message. Every request has a timeout; on timeout the error names the method. [STD, GEN]
3. Notifications from the server are queued in order for the caller. [STD]
4. Requests from the server are answered so it does not wait: `workspace/configuration` with one `null`
   per item (with settings for the server: the value of each item's section, SPEC-0170), `client/registerCapability`, `client/unregisterCapability` and
   `window/workDoneProgress/create` with `null`, anything else with error −32601 (method not found).
   [STD]
5. When the server exits or closes its output, all waiting requests fail at once with an error that
   includes the last lines the server wrote to stderr. [GEN]
6. A server is started from a command with arguments in the workspace root. `initialize` sends the root
   as `rootUri` and workspace folder and the client capabilities used here (diagnostics, hover); then
   `initialized`. `shutdown` then `exit` stop it; a server that does not exit within 2 s is killed. [STD]
7. File paths become `file://` URIs with every byte outside the unreserved set and `/` percent-encoded.
   [STD]
8. Diagnostics of one file: open it (`didOpen` with language id and text), wait for the first
   `publishDiagnostics` for its URI, return severity, 0-based range, message, source and code. [STD]
9. Default servers per language (overridable): C/C++ `clangd`, Rust `rust-analyzer`, Python
   `pyright-langserver --stdio`, TypeScript/JavaScript `typescript-language-server --stdio`, Go `gopls`.
   A missing server is an error that names the command. [GEN]
10. MCP tool `file_diagnostics` (path, optional timeout): diagnostics of one workspace file from its
    language's server. [ADR-0008]

11. **(v3) Features a server does not offer:** a server's error answer with code `-32601` (method
    not found) means that it lacks the feature. The error then reads "`<program>` does not offer
    `<feature>` (`<method>`)" with the server's program name and the feature in words — a call
    hierarchy, a type hierarchy, code actions, renaming, usages, declarations, type declarations,
    implementations, quick documentation, completion, parameter information, a file structure,
    inlay hints, semantic tokens, formatting — instead of the server's raw error text
    ("`textDocument/prepareCallHierarchy`: server error -32601: Unhandled method …"). A server
    that states `renameProvider: false` is not asked to rename; the rename says the same. Other
    error codes are passed on as before. Requests for semantic tokens and hints that fail this
    way are still asked again later (SPEC-0290 behaviour 1a), because a server that is still
    starting may not know a request yet. [STD, OSS, GEN]

12. **(v4) Severity filter:** `file_diagnostics` takes `min_severity` (`error`, `warning`, `information`
    or `hint`) and returns only the findings of that severity or a more severe one, Ferris IDEA's own
    findings included; without it every finding is returned as before. Another value is refused and
    the answer names the four. A finding without a severity counts as `error`, as the protocol says
    the client decides. [STD, GEN; OBS-0071 G4]

13. **(v5) Several files in one call:** `file_diagnostics` takes `paths` instead of `path` — one of
    the two, never both or neither, at most 50 files; a path given twice is answered once. The answer
    has `files` in the order asked, each with its `path` and either its `diagnostics` (the language
    server's and Ferris IDEA's own, as for one file, narrowed by `min_severity`) or its `error`: outside
    the workspace, missing, no server for it (with the command), or no diagnostics within the wait.
    `failed` counts the files with an error. One file's error never fails the call or another file.
    Files served by the same server program share one start of it: all are opened, then the first
    diagnostics published for each are collected within one `timeout_seconds`, whatever order they
    come in; a server that stops gives every file still waiting its error. A server asked through an
    opened document (one that pulls its diagnostics, one that says when it has loaded the project
    such as jdtls, or the window's running servers) is asked file by file as for one file; in the
    window those servers keep running between files. With `path` the answer is unchanged. [STD, GEN;
    OBS-0071 O1]

## Acceptance criteria
- **AK-1** Framing: split and joined messages, extra headers, header case; a body that is not JSON is a
  protocol error.
- **AK-2** With an in-memory fake server: responses are matched by id out of order, response errors keep
  code and message, notifications arrive in order, server requests are answered as in behaviour 4, a
  request times out with the method name.
- **AK-3** When the fake server closes, a pending request fails with the server's last stderr text.
- **AK-4** URIs: spaces, non-ASCII and `#` are percent-encoded.
- **AK-5** With `clangd` installed: a C file with an undeclared identifier gives an error diagnostic at the
  right line; the server is shut down.
- **AK-6** The MCP tool returns diagnostics for a workspace file, refuses paths outside the workspace and
  names a missing server command.

- **AK-7 (v3)** `lsp::tests::an_unknown_request_is_a_feature_the_server_lacks`: `-32601` to
  `textDocument/prepareCallHierarchy` from a server named `some-server` reads "some-server does not
  offer a call hierarchy (textDocument/prepareCallHierarchy)"; an unlisted method reads "this
  request"; code `-32603` stays a server error. With intelephense
  (`crates/languages/tests/it/servers.rs::intelephense_is_asked_after_its_indexing`): a rename
  reads "intelephense does not offer renaming (textDocument/rename)".
- **AK-8 (v4)** MCP: `min_severity` `loud` is refused naming the four values; a missing file is an
  error naming it (`protocol.rs::file_diagnostics_from_the_language_server`); with `clangd`, a C file
  with an error and a `#warning` gives both by default and only the error with `min_severity: error`
  (same test); the filter keeps the order and treats a missing severity as an error (unit test
  `tests::the_severity_filter_keeps_the_more_severe` in `crates/mcp/src/lib.rs`).

- **AK-9 (v5)** `lsp::tests::diagnostics_of_several_files_over_one_connection`: three files are all
  opened before any answer; publications out of order go to their files, a later list does not
  replace the first, and a file without one is a timeout naming `textDocument/publishDiagnostics`.
  MCP (`protocol.rs::file_diagnostics_of_several_files`): `path` with `paths`, neither, an empty
  list and 51 files are refused; four paths with one twice give three entries in order, each with
  its error (no server, outside the workspace, missing) and `failed` 3; with `clangd`, a file with
  an error, a text file, a clean file and a file with a `#warning` give their own results with
  `failed` 1, and `min_severity: error` applies to each file.

## Out of scope
Incremental sync, completion, hover, navigation, code actions and other requests beyond diagnostics
(later M4 steps use the same client); server download/installation; TCP and pipe transports.

## Known deviations from the reference
The reference uses built-in language support, not LSP, for most languages (ADR-0005); behaviour is
compared through results (diagnostics), not protocol.

v4 (OBS-0071): the reference's file analysis over MCP answered seven times in a row with no findings
and only a flag that its time ran out, failed a whole batch for one missing file and left files it
cannot analyse out without a word. `file_diagnostics` answers with the findings or with an error that
names the cause — a missing file, no server (with its command), a server that does not answer within
the wait — and narrows by severity in the same tool (IMP-0441). Open: a server asked through an
opened document that publishes nothing within the wait still gives an empty list (OBS-0071 O9).

v5 (OBS-0071 O1): the reference analyses several files in one call, but one missing file failed the
whole call and a text file was left out of the answer without a word. `file_diagnostics` with `paths`
answers every file asked with its findings or its own error and counts the errors, so nothing is
dropped silently and nothing fails for another file's sake; files of one server share its start
(IMP-0455).
