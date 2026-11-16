# SPEC-0329: Python formatting and import clean-up with the project's tools

- **Version:** 3 · **Date:** 2026-11-12 (v3: carets and selections stay on their text,
  behaviour 7, OBS-0034 item 46; v2: the hint of the import clean-up in safe mode)
- **Sources:** OBS-0034 items 41 and 42 (the reference formats a Python file and removes its
  unused imports without an extra tool; IDECraft refused both: pyright has no formatter, and the
  import clean-up knew Java and Kotlin only), decision of the coordinator 2026-11-12 (no Python
  formatter of IDECraft's own; `ruff`, then `black`, from the selected environment, then from
  `PATH`; nothing downloaded or installed), STD (ruff command line: `ruff format
  --stdin-filename <name> [--range=<l:c-l:c>] -` and `ruff check --select <codes> --fix-only
  --no-cache --stdin-filename <name> -` print the changed text to standard output, rule `F401`
  is the unused import, rule set `I` the import order; black command line: `black -q
  --stdin-filename <name> [--line-ranges=<a-b>] -`; checked against `--help` of ruff 0.17.4 and
  black 25.11.0), STD (The Python Language Reference, "The import statement"), STD (LSP
  `DiagnosticTag.Unnecessary`), SPEC-0043 (plan and preview), SPEC-0050 and SPEC-0117 (the
  selected interpreter and its environment), SPEC-0075 (Reformat Code), SPEC-0122 (Optimize
  Imports), SPEC-0123 (the same approach for Prettier), SPEC-0278 (safe mode), ADR-0008
- **Feature matrix rows:** F-035, F-039

## Purpose
A Python file is laid out and its imports are cleaned the way the project's own tools and its CI
do it, from the editor and for agents, without IDECraft bringing or fetching a tool.

## Behaviour
1. **Lookup:** the formatter of a project is the first that exists of: `ruff` in the folder of
   the selected interpreter (SPEC-0050 behaviour 3; `.venv/bin` and the like), `ruff` on `PATH`,
   `black` in that folder, `black` on `PATH`. The import clean-up looks for `ruff` in the same
   two places. Nothing is downloaded or installed, and no interpreter is started to find a tool.
   [decision, GEN]
2. **Safe mode:** in a project that is not trusted (SPEC-0278) the tool of the selected
   environment is not run — the environment is chosen from the project's content and usually
   lies in it — and `PATH` entries that are relative or lie inside the project are passed over.
   A tool elsewhere on `PATH` runs, as language servers from `PATH` do (SPEC-0278 behaviour 9);
   it reads the project's configuration, which holds settings and no code. Refusing every run in
   safe mode would be the stricter option; the Project Owner decided on 2026-11-12 to allow a
   tool from `PATH`. [SPEC-0278, rule 12, decision]
3. **Nothing found:** the action says which tools it looked for and where, that IDECraft
   installs none itself, and that the Python Packages tool window (MCP:
   `python_install_packages`) adds one to the environment. In safe mode, when only the
   environment has one, it says that the project must be trusted first. [GEN]
4. **Formatting:** the text — the editor's, saved or not — goes to the tool on standard input
   and the result is read from standard output; the tool writes no file. It runs in the project
   root with the file's path relative to it as the file name, so `pyproject.toml`, `ruff.toml`
   and `.ruff.toml` apply as in the project's CI. A selection is formatted with the tool's own
   range option (ruff: lines and columns; black: whole lines). A run has 30 s; a failure (a
   syntax error) is shown with the tool's name and its first line, and nothing changes. The
   result is a plan as in SPEC-0043 behaviour 3 with one edit over the part that differs.
   The indentation options of the editor are not passed on: the project's configuration or the
   tool's defaults decide. [STD, SPEC-0043]
5. **Import clean-up with ruff:** `ruff check --select F401,I --fix-only --no-cache
   --stdin-filename <name> -` removes unused imports and sorts the imports with the project's
   isort settings; rules the project ignores or marks unfixable stay as the project wants them,
   and ruff's own caution applies (it leaves an unused import of an `__init__.py` alone; a
   text it cannot parse comes back unchanged). [STD]
6. **Import clean-up without ruff:** IDECraft removes the imports the Python language server
   marks as not accessed (the diagnostics tagged unnecessary whose place is the bound name of
   an import) and sorts nothing. Only what is safe is changed:
   - only import statements of the module itself, not those in `try`, `if`, functions or
     classes (removing one may leave an empty block), and never `from __future__` or `*`;
   - only a statement that stands alone on its lines and holds no comment (a comment such as
     `# noqa` says the import is wanted);
   - `import a, b` and `from x import a, b` lose their unused names and keep the others as
     written (a list over several lines stays one name per line); a statement with no name left
     goes with its line;
   - an `__init__.py` is refused with the reason: what it imports is usually what the package
     offers.
   What was left is listed with its reason, and the answer says that ruff would also sort.
   [OBS-0034 item 42, STD, GEN]
   **The hint at the end of the summary (v2):** when the imports were not sorted, the one-line
   summary ends with what would sort them. When no ruff exists in the selected environment or
   on the usable `PATH`: "(install ruff to also sort the imports)". When the project is not
   trusted and the selected environment has a ruff that behaviour 2 keeps from running, while
   none is on `PATH` outside the project: "(trust the project to also sort the imports with its
   ruff)" — ruff is installed there and waits for trust, so telling the user to install it
   would be wrong. The same sentence is the status bar's (behaviour 7) and the `summary` of
   `optimize_imports` (behaviour 8). [behaviour 2, GEN]
7. **In the window:** Reformat Code (SPEC-0075) on a Python file uses behaviour 4 for the file
   and for a selection; Optimize Imports (SPEC-0122 behaviour 2) on a Python file uses
   behaviour 5, else 6. Both run in the background, change the editor as one undo step with the
   caret kept on its text, apply nothing when the text changed meanwhile, and say in the status
   bar what was done and with which tool. (v3) The tool's text is applied as small edits, one
   per region of lines that differ, all in one undo step. Every caret and both ends of every
   selection stay on their text: in text no edit touches they move with the edits before them;
   in reformatted text they stay before the same visible character (white space does not count,
   a changed quote or an added comma is skipped); in removed text, such as a removed import, they
   go where that text was. Before v3 the result was one large edit and the caret kept its
   offset, so after Reformat Code a caret in `area` stood in `sides` (OBS-0034 item 46). [GEN, OBS]
8. **MCP:** `format_file` on a Python file answers as in SPEC-0043 and adds `formatter` (`ruff`
   or `black`) and `formatter_path`; `tab_size` and `insert_spaces` have no effect there.
   `optimize_imports` on a Python file returns `changed`, `applied`, `removed`, `left`, `by`
   (`ruff` or `language server`), `sorted`, a one-line `summary` and the new `text`; `dry_run` defaults to true, and
   writing refuses a file that changed meanwhile. [ADR-0008]

## Acceptance criteria
- **AK-1 (behaviours 1–3)** Lookup order, the environment before `PATH` and ruff before black;
  in safe mode the environment's tool, a `PATH` entry inside the project and a relative one are
  passed over; the two messages (unit test `python_tools::tests::lookup_order_and_safe_mode`).
- **AK-2 (behaviours 4–5)** The command lines for a file, a range and the imports (unit test
  `python_tools::tests::command_lines`).
- **AK-3 (behaviour 4)** The plan of a change: one edit in characters, whitespace-only or not,
  none for equal texts (unit test `python_tools::tests::plan_of_a_change`).
- **AK-4 (behaviours 4–5)** With ruff and black installed (`IDECRAFT_TEST_PYTOOLS` names a
  folder with both, else `PATH`; skipped without): unsaved text is formatted and no file is
  written, the project's `pyproject.toml` line length applies, a range leaves the rest alone, a
  syntax error is reported with the tool's name, black formats when ruff is missing, ruff
  removes and sorts imports (`crates/languages/tests/it/python_tools.rs`).
- **AK-5 (behaviour 6)** Whole statements go and others lose their unused names, over several
  lines too; statements in blocks, with comments or sharing a line are left and named
  (`python_tools::tests::{unused_imports_are_removed, unused_imports_that_are_left}`); with
  pyright the three unused imports of a file go (`crates/languages/tests/it/servers.rs`, skipped
  without the server).
- **AK-8 (v2, behaviour 6)** The summary of a clean-up without sorting ends with the hint to
  install ruff when there is none, and with the hint to trust the project when only the
  selected environment of an untrusted project has one; a ruff on `PATH` outside the project
  and a trusted project never give the trust hint (unit tests
  `python_tools::tests::{summaries, ruff_that_waits_for_trust}`). The MCP tool and the
  window pass the same search to the same function; an end-to-end run needs a Python language
  server and is a check by hand (`idecraft call <folder> optimize_imports` in an untrusted
  project whose environment has ruff).
- **AK-6 (behaviour 8)** MCP: `format_file` and `optimize_imports` on a Python file with a
  stand-in formatter on the search path, and the refusals without a tool
  (`crates/mcp/tests/it/protocol.rs`); `idecraft call` on the file of OBS-0034 part 9.
- **AK-9 (v3, behaviour 7)** `idecraft_editor::reformat`: the edits give the formatter's text and
  leave unchanged lines alone; a caret in unchanged text, in reformatted text and in removed text,
  and a selection over a reformatted line, end on the same text (unit tests
  `reformat::tests`).
- **AK-7 (behaviour 7)** In the window, by hand next to the reference: Reformat Code and
  Optimize Imports on that file, a selection, undo, the messages without a tool.

## Out of scope
A Python formatter of IDECraft's own; installing a tool from the action; formatting on save;
other formatters (yapf, autopep8) and other import sorters (isort alone); passing the editor's
indentation or the code style settings to the tool; notebooks; a remote project's host; sorting
imports without ruff; unused imports inside blocks without ruff.

## Known deviations from the reference
The reference formats Python with a built-in formatter and its own code style settings, without
any tool, and keeps for example `import os, sys` on one line, one-line bodies and the quotes as
written (OBS-0034 item 41). IDECraft runs the project's formatter instead, so Reformat Code gives
exactly what the project's CI check accepts (IMP-0401); the price is that a project without ruff
or black is not formatted until one is installed, which the action says. Without ruff the import
clean-up removes but does not sort, and leaves imports inside blocks.
