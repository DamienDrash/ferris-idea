# SPEC-0090: Language server pool

- **Version:** 2 · **Date:** 2026-11-05 (v2: a request the server rejects because the document
  changed meanwhile is asked again)
- **Sources:** GEN (IDEs keep their code analysis running for an open project: errors appear while
  typing, completion and navigation answer at once; F-031, F-032), STD (LSP 3.17 document
  synchronisation: `didOpen`, `didChange` with full text, `didClose`, `publishDiagnostics` pushed by the
  server; `shutdown`/`exit`), SPEC-0023 (LSP client), SPEC-0042 (server readiness quirks), SPEC-0070
  (editor diagnostics), ROADMAP M8.3 (pulled forward)
- **Feature matrix rows:** F-031, F-032

## Purpose
Ask a language server about code without starting it each time, and see its errors while typing, not
only after saving.

## Behaviour
1. **One server per workspace and program:** with the pool on, the first question about a file starts
   its language server in the workspace, initialises it and waits until it reports readiness where it
   does so (SPEC-0042 v2); later questions of any kind (hover, completion, definition, rename, format,
   code actions, symbols, call hierarchy, diagnostics) reuse it. A server that stopped is started again on
   the next question. Without the pool (library use, tests) every question starts and stops a server as
   before. [STD]
2. **Documents stay open:** a file is opened on the server once (`didOpen`, version 1) and kept open;
   when a question or the editor brings a different text, the whole new text is sent as `didChange` with
   the next version. The same text sends nothing. The server's quirks apply once per document: the resent
   text after opening, the probe until loading has finished and the wait for the first diagnostics
   (SPEC-0042 v3, v4). [STD]
3. **Diagnostics are pushed:** every `publishDiagnostics` of a pooled server is kept per file with an
   increasing update number; `diagnostics_since(n)` returns the files updated after `n` with their
   current diagnostics. A question for diagnostics on a text the server already has answers with what it
   published for it. [STD]
4. **Editor:** the IDE window switches the pool on. Every 300 ms it sends the text of each editor whose
   content changed since the last sending to the file's server, closes documents of closed tabs
   (`didClose`) and shows the diagnostics published for the text the server has; diagnostics that arrive
   while the editor has newer text are skipped, the newer text's diagnostics follow. Opening and saving
   still ask for diagnostics as in SPEC-0070. [GEN]
5. **MCP:** `ferris-idea mcp` switches the pool on too, so agents asking several questions about a project
   reuse its servers. [ADR-0008]
6. **Lifetime:** quitting the IDE or ending the MCP session sends `shutdown` and `exit` to every pooled
   server and kills those that have not exited after 2 s; a server also sees its input closed when the
   IDE process ends. Servers run on the pool's own thread, so a question's short-lived runtime may end
   without taking the server's connection with it. [STD]
7. **Changed meanwhile (v2):** a server may reject a request with the LSP errors `ContentModified`
   (−32801) or `ServerCancelled` (−32802) when the document changed or its analysis restarted while the
   request was pending — rust-analyzer does so while it is still loading a project. The client asks the
   same request again up to four more times, waiting 100, 200, 400 and 800 ms, against the current state
   of the document; only when the last attempt is rejected too does the caller see the error. Other
   errors and timeouts are not repeated. [STD: LSP 3.17 error codes; found in CI, PR #327]

## Acceptance criteria
- **AK-v2** Unit test with a fake server: a request answered twice with −32801 and then with a result
  returns the result; one answered five times with −32801 returns the error after five attempts; a
  −32000 error is returned at once.
- **AK-1** With `clangd`: two hovers start one server and the second is faster; an unsaved text with an
  undeclared identifier yields its error through `diagnostics_since` without saving, and the fixed text
  clears it; completion on the unsaved text uses the same server; after `shutdown` none runs
  (`tests/pool.rs`).
- **AK-2** All language tests keep passing without the pool (`tests/servers.rs`, `tests/clangd.rs`,
  `tests/rust_analyzer.rs`).
- **AK-3** In a real window (`ferris-idea ui --selftest`, with `clangd`): the self-test's diagnostics steps
  pass with the pool on.
- **AK-4** By hand: typing an error in a C++ file underlines it within about a second without saving;
  fixing it removes the underline.

## Out of scope
Incremental `didChange` ranges (whole text is sent; fine for files of normal size), pull diagnostics
(`textDocument/diagnostic`), several workspace folders per server, a status widget listing running
servers, memory limits for idle servers.

## Known deviations from the reference
The reference analyses with its own engine; Ferris IDEA relies on the language servers. Diagnostics follow
typing with up to 300 ms plus the server's own delay.
