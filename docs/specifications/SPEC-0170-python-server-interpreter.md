# SPEC-0170: The Python language server uses the selected interpreter

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (an IDE resolves imports against the project's interpreter and its installed
  packages, and follows a change of interpreter without a restart; F-035), STD (LSP 3.17
  `workspace/configuration` with dotted `section` names and `workspace/didChangeConfiguration`;
  pyright/basedpyright settings `python.pythonPath`, `python.venvPath`, `python.analysis.*` and the
  project files `pyrightconfig.json` and `[tool.pyright]` in `pyproject.toml` with `venvPath`/`venv`,
  as documented by the pyright project), SPEC-0023 (client), SPEC-0042 (server register), SPEC-0050
  and SPEC-0117 (selected interpreter), SPEC-0090 (pool)
- **Feature matrix rows:** F-035

## Purpose
Imports in Python files resolve against the environment the project runs with (the interpreter in
the status bar), not against whatever `python` pyright finds on `PATH`, and switching the interpreter
takes effect in the open editor at once.

## Behaviour
1. The LSP client keeps a settings object per server. A `workspace/configuration` request is
   answered item by item: an item with `section` gets the value at that dotted path (`python`,
   `python.analysis`), an item without `section` the whole object, a missing path `null`. Without
   settings every item is `null` as before (SPEC-0023 behaviour 4). [STD]
2. For `pyright-langserver` and `basedpyright-langserver` the settings are
   `{"python": {"pythonPath": <interpreter>}}` with the interpreter selected for the workspace
   (SPEC-0050 behaviour 3, including the choice saved by SPEC-0117), as an absolute path. Other
   servers get no settings. When no interpreter is found the `python` section stays empty and the
   server picks its own default. [STD, GEN]
3. The settings are in place before `initialize`, so the first analysis already uses the
   interpreter. [STD]
4. When the selected interpreter of a workspace changes (a new choice, a new `.venv`, the
   automatic choice restored), every running pyright/basedpyright server of that workspace gets the
   new settings and `workspace/didChangeConfiguration` with them; the server asks for its
   configuration again and analyses open files anew. Nothing is restarted. [STD]
5. A project file that names an environment (`venv` in `pyrightconfig.json` or in
   `[tool.pyright]` of `pyproject.toml`) is the project's decision: the settings are sent unchanged,
   and the report (behaviour 6) says that the project file decides. [STD]
6. MCP tool `python_language_server` reports the server that would be used (program and path, or
   why there is none), the settings sent, the interpreter and how it was selected, whether a project
   file names an environment, and whether a server runs for the workspace. With `apply: true` it
   sends the current settings to running servers (behaviour 4). [ADR-0008]

## Acceptance criteria
- **AK-1** Configuration answers by dotted section, whole object and missing path; settings for
  pyright/basedpyright and none for others; project-file detection (unit tests).
- **AK-2** A real pyright with a project whose `.venv` holds a package that the system interpreter
  lacks: with the venv selected an `import` of it gives no unresolved-import diagnostic; with a
  second, empty venv selected and the change pushed to the running server the diagnostic appears
  (integration test; skipped cleanly without pyright or `python3 -m venv`).
- **AK-3** The MCP tool reports server, settings, interpreter and the project-file note.
- **AK-4** Window: choosing another interpreter pushes the settings to the running server (self-test
  checks that the push is made for the workspace).
- **AK-5** Hand test: an import that resolves only in `.venv` is clean in the editor; switching to an
  interpreter without the package marks it, switching back clears it, without reopening the file.

## Out of scope
Per-folder interpreters in one workspace, stub packages (`typeshed` paths), `extraPaths` settings,
remote interpreters, the reference's own Python analysis.

## Known deviations from the reference
The analysis comes from pyright/basedpyright, not from the reference's own engine; not compared
side by side yet.
