# SPEC-0117: Python interpreter and packages in the window

- **Version:** 5 · **Date:** 2026-11-10 (v2: the widget follows the active editor; v3: the language server reads the environment again after an install, hand test Nr. 44, D25; v4: ⌥↩ on an unresolved import installs its package, OBS-0034 L2; v5: installs use the installer of SPEC-0171 v2, so an environment made by uv works, and a failed install is always visible, OBS-0034 L3 and L4)
- **Sources:** GEN (IDEs show the project's Python interpreter in the status bar, let the user switch
  it or create a virtual environment, and list and install packages in a tool window; F-035), OSS
  (tool window name "Python Packages" and its activation action id), STD (`python -m venv` (PEP 405),
  `python -m pip install` with PEP 508/440 requirements, PEP 668 for shared interpreters),
  SPEC-0050 (interpreters, packages, unmet declarations), SPEC-0048 (declared dependencies),
  SPEC-0053 (agent `run` right and log), ADR-0008, OBS-0034 (L2), OSS (v4: the open Python plugin
  offers, in the quick-fix list of an unresolved import, to install the package, and to install a
  declared requirement), SPEC-0076 (the ⌥↩ list), SPEC-0170 (pyright and its rule names); v5:
  OBS-0034 (L3, L4), SPEC-0171 v2 (the installer), SPEC-0070 v9 (status bar: a held failure)
- **Feature matrix rows:** F-035

## Purpose
See and change which Python a project runs with, and fix what the environment lacks, without a
terminal.

## Behaviour
1. **Status bar:** in a Python project (a Python module in the project model, or `.py` files) the
   status bar shows the selected interpreter, e.g. `Python 3.12 (.venv)`; it is read in the
   background when the workspace opens with `pyproject.toml`, `requirements.txt`, `setup.py` or
   `setup.cfg`, and again after a Python file or declaration is saved while it is unknown or the
   tool window is open. [GEN, SPEC-0050]
   - **Follows the active editor (v2):** the widget is shown while the active tab holds a Python
     file (`.py`, `.pyi`) or a Python declaration (`pyproject.toml`, `requirements*.txt`, `setup.py`,
     `setup.cfg`, `Pipfile`); with another file active (Java, TypeScript, …) it is hidden, so the
     status bar never names Python for a file of another language. With no editor open it is shown
     in a Python project. (Hand test 2026-11-04: `test.java` active, the bar showed Python because the
     widget only asked whether the project had `.py` files.) [GEN]
2. **Choosing:** a click on the widget or `Python.SelectInterpreter` lists every interpreter found
   (version, kind, how it was found, path; the selected one marked), then "Automatic choice" and
   "New virtual environment in .venv". A choice is saved in `.idecraft/python.toml`
   (workspace-relative inside the workspace) and used by SPEC-0050 as the configured interpreter;
   the automatic choice removes the file. [GEN]
3. **New environment:** `Python.CreateVirtualEnv` (also from the chooser and the tool window) runs
   `<base> -m venv .venv` in the background, the base being the selected interpreter or, for an
   environment, its `home`; on success the new interpreter is chosen. An existing `.venv` is left
   alone. [STD]
4. **Tool window:** `ActivatePythonPackagesToolWindow` shows "Python Packages": the interpreter as a
   button that opens the chooser, the reason and notes of the selection, declared dependencies the
   environment does not satisfy (warning colour, with requirement and installed version), then the
   installed packages sorted by name. Escape hides it; ⇧Esc hides it like other bottom windows.
   [GEN, OSS]
5. **Installing:** a field takes requirements separated by spaces (`requests>=2.31 rich`), Enter
   installs them; Enter on an unsatisfied row installs that one; "Install missing (n)" installs all.
   Installation runs with the installer of SPEC-0171 v2 behaviour 4 (v5; before, always pip):
   `uv pip install --python <python> …` when the project uses uv, or the selected environment was
   made by uv or has no pip, and uv is found; else
   `<python> -m pip install --disable-pip-version-check …`; never `poetry add`. It runs with the
   selected interpreter, without a shell and without prompts, in the background; the status bar
   reports the last line of the installer's output or the failure (behaviour 9), and the window
   reads the environment again. Poetry `^`/`~` and `*` requirements are installed by name only. A
   plain system or pyenv interpreter is refused (IMP-0042), and so is an environment without pip
   when uv is not found (SPEC-0171 v2 behaviour 4); nothing is started then. [STD, OBS-0034]
6. **MCP:** `python_select_interpreter` (`interpreter`, omitted for automatic; `dry_run` default
   true), `python_create_venv` (`path` default `.venv`, workspace-relative; `dry_run` default true;
   running needs the `run` right), `python_install_packages` (`packages`, omitted for every unmet
   declaration; `dry_run` default true; running needs the `run` right; v5: the preview's
   `would_run` is the command that will really run, with `installer` (`uv` or `pip`) and `notes`
   (why this installer), and both refusals of behaviour 5 are errors in the preview too). Runs are logged in
   `.idecraft/agent-log.jsonl`. `python_environments` uses the saved choice unless an interpreter is
   given. [ADR-0008, SPEC-0053]
7. **Imports follow an install (v3, D25):** pyright reads the environment's packages when it starts
   and does not notice new ones, so `import rich` stayed marked as unresolved after `rich` was
   installed. After a pip or venv command succeeds (window or MCP `python_install_packages`), the
   project's Python language servers are restarted and the open Python files are checked again, so
   the marks follow the environment without reopening the project. [GEN, STD]
8. **Installing from an unresolved import (v4, OBS-0034 L2):** with the caret on a line of a Python
   file where the language server marks an import as unresolved (pyright's rules
   `reportMissingImports` and `reportMissingModuleSource`), ⌥↩ lists first, before IDECraft's
   other fixes and the language server's actions:
   - `Install package <distribution>` for each module marked on that line. The marked text is the
     module (`yaml`, `google.cloud.storage`, the module after `from`); its first part is the import
     name. Relative imports (`from . import x`) get no entry. The distribution is found in this
     order: (a) a declared dependency the environment does not satisfy whose PEP 503 name equals
     the import name or the distribution of (b); it is installed with its declared requirement
     (`PyYAML>=6`, built as in behaviour 5); (b) a small table of well-known distributions whose
     import differs: `yaml` PyYAML, `PIL` Pillow, `cv2` opencv-python, `sklearn` scikit-learn,
     `bs4` beautifulsoup4, `dateutil` python-dateutil, `dotenv` python-dotenv, `jwt` PyJWT,
     `serial` pyserial, `usb` pyusb, `Crypto` pycryptodome, `zmq` pyzmq, `OpenSSL` pyOpenSSL,
     `MySQLdb` mysqlclient, `docx` python-docx, `pptx` python-pptx, `magic` python-magic,
     `attr` attrs, `google.protobuf` protobuf; (c) the import name. Two modules of the same
     distribution give one entry.
   - `Install all missing requirements (n)` when the environment leaves two or more declared
     dependencies unsatisfied: it installs all of them, as "Install missing (n)" in the tool window.

   Choosing an entry installs through behaviour 5: the selected interpreter, the same pip command,
   progress and then the last line of pip's output or the failure in the status bar (and in the
   tool window when it is open), the shared-interpreter refusal of IMP-0042, nothing while another
   pip or venv command runs, and afterwards behaviour 7 (the language server reads the environment
   again and the open Python files are checked again). Without a known environment the entries
   come from (b) and (c), and choosing one says that no interpreter is selected. [OSS, GEN, STD]
   - **MCP:** `python_install_packages` takes `imports` (module names as written in the import,
     e.g. `["yaml", "requests"]`) instead of or next to `packages`; they are mapped as above, and
     the answer lists the mapping (`import`, `package`, `spec`, `source`: `requirement`, `known` or
     `import`) next to the command, in the preview too. Rights, trust and the interpreter refusal
     are those of `packages`. [ADR-0008]
9. **A failed install is visible (v5, OBS-0034 L4):** when an install or venv command of this
   window, of the import quick fix or of the index install (SPEC-0171 behaviour 6) ends with a
   failure, cannot be started, or is refused before it starts (no interpreter, a shared
   interpreter, no pip and no uv):
   - **Window:** "Python Packages" opens if it is hidden (the keyboard focus stays where it is, so
     typing continues in the editor) and shows the failure above the list, framed in the error
     colour: a title (`Command failed (exit code n)`, `Command could not be started`, or
     `Not installed: <requirements>` for a refusal), the command line that ran after `$`, and the
     last 12 lines of its output (for a refusal: the reason with its remedies, and no command).
     The block stays until the next install or venv command starts.
   - **Status bar:** the failure (`Failed: <last output line>`, or `Not installed: <reason>`) is
     held in front of the problem at the caret as SPEC-0070 v9 behaviour 4a defines, so the
     pyright message of the very import whose quick fix failed does not cover it.
   A successful command reports as before and opens nothing. [GEN, OBS-0034]

## Acceptance criteria
- **AK-1** Saving and clearing the choice, labels, chooser lines, base interpreter, venv and pip
  commands, pip requirements from declarations, command runs (unit tests `python_manage::tests`,
  `python_view::tests`).
- **AK-2** A real `python3 -m venv` made with the generated command is chosen and found as
  configured, labelled with its folder, lists `pip`; an offline install of an unknown package fails
  with pip's message (`crates/core/tests/it/python.rs`).
- **AK-3** MCP: preview and save a choice, install preview with the unmet declaration, venv preview,
  refusals for an existing or outside folder and for running without the `run` right, clearing
  (`crates/mcp/tests/it/protocol.rs`: `python_manage_tools`).
- **AK-4** Window self-test: the chosen environment in the status bar, the tool window rows, the
  chooser lines, Escape, a failing install reported, the automatic choice restored.
- **AK-5** Hand test (round 4): a project with `pyproject.toml`; create `.venv`, install a missing
  dependency, switch interpreters, next to the reference.
- **AK-7 (v3)** With pyright: an import of a package added to the environment stays unresolved
  until the restart and resolves after it (live test
  `tests/python_server.rs::pyright_follows_the_selected_interpreter`, fails without the restart).
- **AK-8 (v4)** Import-to-distribution mapping: the declared requirement first (also through the
  table and PEP 503 spelling), the table, the import name, dotted and relative imports, the entries
  with "Install all missing requirements (n)" and without it for a single one, duplicates merged
  (unit tests `python_manage::tests`).
- **AK-9 (v4)** Quick-fix entries from the editor's problems: only the two pyright rules, only on
  the caret's line, the module read from the marked text, nothing for other rules (unit tests
  `python_fixes::tests`); window self-test: ⌥↩ on `import idecraft_demo_pkg` offers
  `Install package idecraft-demo-pkg` first, Enter installs the declared requirement through
  behaviour 5 and its failure is reported.
- **AK-10 (v4)** MCP: `python_install_packages` with `imports` previews the mapped command with the
  mapping (`crates/mcp/tests/it/protocol.rs`: `python_manage_tools`).
- **AK-11 (v5)** In a window with a fixture environment made the uv way (no pip) and a stub `uv`
  on an injected `PATH` that fails: after the install of the quick fix the hidden Python Packages
  window is open with the title, the uv command line and the stub's last output line; the status
  bar shows `Failed: …` although the caret is on pyright's problem, shows the problem again after
  the caret moved (also back at the old place), and the failure again off the problem. With an
  empty `PATH`: nothing runs, the window shows `Not installed: requests` with the reason naming
  `-m ensurepip` and installing uv (`crates/workbench/tests/it/python_install_failure.rs`); the
  failure block's lines and the 12-line tail (unit test `python_view::tests`).
- **AK-12 (v5)** Window self-test with stubs: after pip is taken out of the fixture environment,
  ⌥↩ → Enter with a stub `uv` on the injected `PATH` runs
  `uv pip install --python … idecraft-demo-pkg>=1`; its failure shows in the window and in the
  status bar over the import's problem.
- **AK-6 (v2)** Visibility by active file: shown for Python files and declarations, hidden for a
  Java or TypeScript file, shown with no editor in a Python project, never outside one (unit test
  `python_widget::tests`); hand test 207.

## Out of scope
Uninstalling and upgrading packages, searching the package index, conda/Poetry/Pipenv-managed
environments as targets for installing (an environment made by `uv venv` is a target since v5),
remote and WSL interpreters, passing the interpreter to the
Python language server, per-module interpreters.

## Known deviations from the reference
Requirements are typed instead of searched in the package index. The import quick fix installs a
declared requirement with its version constraint rather than the bare name, and is refused for a
shared interpreter (IMP-0042). Shared interpreters are refused
for installing (IMP-0042). Not compared side by side yet.
