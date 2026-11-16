# Specifications

Technology-neutral functional specifications (clean-room phase B). A spec
describes **behaviour**. For proprietary reference features it never contains
foreign code, text or visual design; for open-source parts it may cite the
source (`OSS`), with ported material recorded in `docs/legal/PROVENANCE.md`.
Rules: [`../legal/RESEARCH_BOUNDARIES.md`](../legal/RESEARCH_BOUNDARIES.md).

| Spec | Title | Sources | Status |
|---|---|---|---|
| [SPEC-0001](SPEC-0001-workspace.md) | Workspace | GEN | implemented |
| [SPEC-0002](SPEC-0002-layered-settings.md) | Layered settings | GEN | implemented |
| [SPEC-0003](SPEC-0003-text-buffer.md) | Text buffer with undo/redo | GEN, STD (Unicode) | implemented |
| [SPEC-0004](SPEC-0004-language-identification.md) | Language identification | GEN, STD (LSP) | implemented |
| [SPEC-0005](SPEC-0005-sql-script-splitting.md) | SQL script splitting | GEN, STD (SQL, PostgreSQL/MySQL/SQLite docs) | implemented |
| [SPEC-0006](SPEC-0006-mcp-server.md) | MCP server | STD (MCP), OSS (logic template) | implemented |
| [SPEC-0007](SPEC-0007-script-results-and-stable-order.md) | Script results, error positions, stable order | GEN, STD, OBS-0004/0005 | implemented (logic) |
| [SPEC-0008](SPEC-0008-connection-profiles.md) | Connection profiles | GEN, STD (libpq/MySQL URIs, RFC 3986), OBS-0004 | implemented |
| [SPEC-0009](SPEC-0009-postgresql-session.md) | PostgreSQL session | STD (PostgreSQL protocol), OBS-0004, ADR-0011 | implemented |
| [SPEC-0010](SPEC-0010-mysql-session.md) | MySQL session | STD (MySQL protocol), OBS-0004, ADR-0011 | implemented |
| [SPEC-0011](SPEC-0011-sqlite-session.md) | SQLite session | STD (SQLite C interface), ADR-0011 | implemented |
| [SPEC-0012](SPEC-0012-sql-over-mcp.md) | SQL over MCP with permissions | STD, OBS-0004, ADR-0008, ADR-0031 | implemented (v3: permissions granted by the user outside the project) |
| [SPEC-0013](SPEC-0013-schema-introspection.md) | Schema introspection | STD (catalogs of PostgreSQL, MySQL, SQLite), OBS-0004 | implemented |
| [SPEC-0014](SPEC-0014-sql-completion.md) | Schema-aware SQL completion | GEN, STD | implemented (logic + MCP) |
| [SPEC-0018](SPEC-0018-schema-diff.md) | Schema comparison and DDL generation | GEN, STD (DDL per dialect, SQLite table rebuild), SPEC-0013 | implemented (logic + MCP) |
| [SPEC-0019](SPEC-0019-sql-rename.md) | Renaming tables and columns in SQL | GEN, STD (ALTER TABLE RENAME), SPEC-0005, SPEC-0014 | implemented (logic + MCP) |
| [SPEC-0020](SPEC-0020-query-plan.md) | Query plans | GEN, STD (EXPLAIN per dialect), SPEC-0012 | implemented (logic + MCP) |
| [SPEC-0021](SPEC-0021-data-export.md) | Exporting result data | GEN, STD (RFC 4180, JSON), OBS-0005, IMP-0013 | implemented (logic + MCP) |
| [SPEC-0022](SPEC-0022-er-model.md) | Entity-relationship model | GEN, STD (Mermaid, DOT, GraphML, draw.io), SPEC-0013 | implemented (logic + MCP) |
| [SPEC-0023](SPEC-0023-lsp-client.md) | Language Server Protocol client | STD (LSP 3.17, JSON-RPC 2.0, RFC 8089), ADR-0005 | implemented (diagnostics + MCP) |
| [SPEC-0024](SPEC-0024-code-insight.md) | Code insight over LSP | STD (LSP 3.17 hover, definition, references, completion, symbols, position encodings), SPEC-0023 | implemented (logic + MCP) |
| [SPEC-0025](SPEC-0025-selections.md) | Selections and multiple carets | GEN, SPEC-0003 | implemented (logic + MCP) |
| [SPEC-0026](SPEC-0026-file-watching.md) | Watching workspace files | GEN, STD (file metadata), SPEC-0001, SPEC-0003 | implemented (logic + MCP) |
| [SPEC-0027](SPEC-0027-syntax-highlighting.md) | Syntax highlighting with Tree-sitter | GEN, STD (Tree-sitter, highlight queries; grammars MIT) | implemented (logic + MCP) |
| [SPEC-0028](SPEC-0028-keymaps.md) | Keymaps | OSS (intellij-community keymaps, Apache-2.0, ADR-0009), STD (KeyStroke text) | implemented (logic + MCP) |
| [SPEC-0029](SPEC-0029-find-replace.md) | Find and replace in a file and in the workspace | GEN, STD (`regex` crate syntax), SPEC-0001 | implemented (logic + MCP) |
| [SPEC-0030](SPEC-0030-folding.md) | Code folding | GEN, STD (Tree-sitter, LSP folding kinds), SPEC-0027 | implemented (logic + MCP) |
| [SPEC-0031](SPEC-0031-search-everywhere.md) | Search everywhere — files, actions, symbols | GEN, STD (Tree-sitter tags queries), SPEC-0028 | implemented (logic + MCP) |
| [SPEC-0032](SPEC-0032-git-basics.md) | Git — status, history and changes of a file | GEN, STD (Git via `gix`, diff via `similar`) | implemented (logic + MCP) |
| [SPEC-0033](SPEC-0033-git-blame-branches.md) | Git — blame and branches | GEN, STD (`gix-blame`, Git references) | implemented (logic + MCP) |
| [SPEC-0034](SPEC-0034-git-commit.md) | Git — commit selected files and create branches | GEN, STD (Git objects, index, references via `gix`) | implemented (logic + MCP) |
| [SPEC-0035](SPEC-0035-git-switch-branch.md) | Git — switch branches | GEN, STD (`git switch` semantics via `gix`) | implemented (logic + MCP) |
| [SPEC-0036](SPEC-0036-git-merge.md) | Git — merge a branch into the current one | GEN, STD (`gix-merge`, Git merge semantics) | implemented (logic + MCP) |
| [SPEC-0037](SPEC-0037-three-way-merge.md) | Three-way text merge (merge editor model) | GEN, STD (diff3, Git conflict markers) | implemented (logic + MCP) |
| [SPEC-0038](SPEC-0038-terminal.md) | Integrated terminal — PTY session and screen model | GEN, STD (ECMA-48, xterm sequences; `vte`, `portable-pty`) | implemented (logic + MCP render) |
| [SPEC-0039](SPEC-0039-debugger-dap.md) | Debugger — DAP client and session model (v2: conditional breakpoints, logpoints) | GEN, STD (Debug Adapter Protocol; `lldb-dap` for tests) | implemented (logic + MCP adapter list) |
| [SPEC-0040](SPEC-0040-lsp-rename.md) | Rename refactoring through the language server | GEN, STD (LSP rename, workspace edits) | implemented (logic + MCP) |
| [SPEC-0041](SPEC-0041-lsp-code-actions.md) | Quick fixes and refactorings through the language server | GEN, STD (LSP code actions, executeCommand, applyEdit) | implemented (logic + MCP) |
| [SPEC-0042](SPEC-0042-language-servers.md) | Language server registry | GEN, STD (servers' command lines, rustup proxies) | implemented (logic + MCP) |
| [SPEC-0043](SPEC-0043-lsp-formatting.md) | Code formatting through the language server | GEN, STD (LSP formatting, range formatting) | implemented (logic + MCP) |
| [SPEC-0044](SPEC-0044-local-history.md) | Local history | GEN, DOC, STD (SHA-256, JSON Lines) | implemented (logic + MCP) |
| [SPEC-0045](SPEC-0045-test-reports.md) | Test results — reading test reports into a result tree | GEN, STD (JUnit XML, TAP, libtest output) | implemented (logic + MCP) |
| [SPEC-0046](SPEC-0046-http-client.md) | HTTP client — request files and sending | GEN, STD (RFC 9110/9112), OSS (`.http` convention) | implemented (logic + MCP parse) |
| [SPEC-0047](SPEC-0047-run-configurations.md) | Run configurations | GEN, own TOML format | implemented (logic + MCP list) |
| [SPEC-0048](SPEC-0048-project-model.md) | Project model from build files | GEN, STD (Cargo, npm, PEP 621/508, Poetry, Maven POM with parents, go.mod, Gradle scripts and catalog, lock files) | implemented (logic + MCP) |
| [SPEC-0049](SPEC-0049-idea-project-files.md) | Project files of the reference IDE (read only) | OSS (directory-based project format of the open platform), OBS-F | implemented (logic + MCP) |
| [SPEC-0050](SPEC-0050-python-environments.md) | Python interpreters and environments | GEN, STD (PEP 405, 376/627, 503, 440; pyenv, conda) | implemented (logic + MCP) |
| [SPEC-0051](SPEC-0051-node-environment.md) | Node.js runtimes, package manager and packages | GEN, STD (package.json, node-semver ranges, lockfiles, nvm/fnm) | implemented (logic + MCP) |
| [SPEC-0052](SPEC-0052-java-environment.md) | Java development kits and the project's Java version | GEN, STD (JDK `release` file, Maven, Gradle, jenv, SDKMAN) | implemented (logic + MCP) |
| [SPEC-0053](SPEC-0053-agent-permissions.md) | Agents running programs and sending requests | GEN, OSS (MCP plugin, logic template), decision PO question 3 | implemented (logic + MCP) |
| [SPEC-0054](SPEC-0054-debugging-through-mcp.md) | Debugging through MCP | STD (DAP), SPEC-0039, SPEC-0053 | implemented (logic + MCP) |
| [SPEC-0055](SPEC-0055-git-remotes.md) | Git remotes — tracking, fetch, pull, push | GEN, STD (git command line) | implemented (logic + MCP) |
| [SPEC-0056](SPEC-0056-editor-actions.md) | Editing actions | GEN, SPEC-0003/0025/0004 | implemented (logic + MCP) |
| [SPEC-0015](SPEC-0015-sql-inspections.md) | SQL inspections | GEN, STD (MySQL manual) | implemented (logic + MCP) |
| [SPEC-0016](SPEC-0016-tls.md) | TLS for PostgreSQL and MySQL | STD (libpq sslmode, MySQL ssl-mode), ADR-0011 | implemented |
| [SPEC-0017](SPEC-0017-data-editing.md) | Editing table data | GEN, STD (NULL-safe comparison, transactions), OBS-0005, IMP-0014 | implemented (logic + MCP) |

Weaknesses of the reference are improved, not copied:
[`IMPROVEMENTS.md`](IMPROVEMENTS.md) lists each one with the IDECraft behaviour.

New specs start from [`TEMPLATE.md`](TEMPLATE.md). Status of implementation and
tests: [`TRACEABILITY.md`](TRACEABILITY.md).
