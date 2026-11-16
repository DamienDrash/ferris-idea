# SPEC-0050: Python interpreters and environments

- **Version:** 3 · **Date:** 2026-11-16 (v3: the reference's environment answer for a file that is not Python named as a deviation, IMP-0449, OBS-0071); 2026-11-10 (v2: every Python launch uses the selected interpreter, OBS-0034 L1)
- **Sources:** GEN (IDEs find Python interpreters and virtual environments, pick one per project, show its
  installed packages, warn about unmet requirements and run tests with it; F-035), STD (`venv` and
  `pyvenv.cfg` (PEP 405), installed distributions as `*.dist-info` (PEP 376/627) with core metadata
  `Name`/`Version`, legacy `*.egg-info`, name normalisation (PEP 503), version specifiers (PEP 440),
  Poetry's caret and tilde requirements, pyenv's `versions/` and `.python-version`, conda's `conda-meta`,
  the `VIRTUAL_ENV` and `CONDA_PREFIX` variables), SPEC-0048 (declared dependencies and `requires-python`),
  SPEC-0047 (run configurations), SPEC-0045 (JUnit reports), SPEC-0087 (run window, a start that cannot happen),
  SPEC-0091 (debugging with debugpy), SPEC-0097 (run the current file), SPEC-0117 (the stored choice), OBS-0034
- **Feature matrix rows:** F-035

## Purpose
Know which Python a project runs with, whether that environment matches what the project declares, and
how to run its tests, without executing anything to find out.

## Behaviour
1. Interpreters are found from: a configured interpreter (file or environment folder), the active
   `VIRTUAL_ENV`, virtual environments in the workspace (direct sub-folders with `pyvenv.cfg`; `.venv` and
   `venv` first), the active `CONDA_PREFIX`, pyenv versions (newest first) and `python3`/`python` on
   `PATH`. Unix (`bin/`) and Windows (`Scripts/`) layouts are recognised. Duplicates (same resolved file;
   virtual environments by their own path) are dropped. [STD]
2. Each interpreter has path, kind (`venv`, `conda`, `pyenv`, `system`), version, base interpreter
   (`home` of `pyvenv.cfg`), whether it lies in the project and how it was found. The version comes from
   `pyvenv.cfg` (`version` or `version_info`), the pyenv folder name, the executable name (`python3.12`) or
   the `lib/pythonX.Y` folder of its prefix. **No interpreter is started**; an unknown version is reported
   as such. [STD, improvement IMP-0025]
3. Selection order: configured, active `VIRTUAL_ENV`, project environment, the pyenv version named in
   `.python-version` (prefix match), active conda environment, first on `PATH`, newest pyenv version. The
   reason is reported. [GEN]
4. Installed distributions of the selected environment are read from its `site-packages`
   (`lib/python*/site-packages` or `Lib/site-packages`): `*.dist-info` and `*.egg-info`, name and version
   from `METADATA`/`PKG-INFO`, else from the folder name; names normalised (PEP 503). [STD]
5. Checks against the project (SPEC-0048): the selected version against `requires-python`; every declared
   dependency against the installed version (PEP 440 operators `==`, `!=`, `<`, `<=`, `>`, `>=`, `~=`,
   `===`, wildcards `==X.*`, Poetry `^` and `~`; extras ignored). Missing and non-matching dependencies are
   listed with the installed version. Dependencies with environment markers are listed as not checked;
   `path`/`git`/`workspace` dependencies are skipped. A plain system interpreter is not checked. [STD]
6. Test framework: `pytest` when installed or declared, else `unittest`. A suggested run configuration
   (SPEC-0047) uses the selected interpreter (`${workspace}`-relative inside the project): pytest as a test
   configuration writing a JUnit report to `.idecraft/pytest.xml`, unittest as `-m unittest -v`. Nothing
   is written automatically. [GEN]
7. MCP tool `python_environments` (optional `interpreter`) with the above, reading `requires-python` and
   dependencies from the root module of SPEC-0048. [ADR-0008]
8. **v2 – Every Python launch uses the selected interpreter.** The workspace's selected interpreter
   (behaviour 3, including the choice stored by SPEC-0117) is resolved in one place and used by everything
   IDECraft starts with Python: Run current file and the ▶ markers of a Python file (`python <file>`), a test
   file or test method (`python -m pytest …`), the pytest configuration suggested by the project model
   (SPEC-0048), the test configuration of behaviour 6, and debugging a Python configuration with debugpy
   (SPEC-0091: the interpreter of the debugged program). [GEN, OBS-0034 L1]
   - A configuration whose command starts with a bare `python` or `python3` — generated ones and those
     stored in `.idecraft/run.toml` — runs with the selected interpreter. A command that names a path
     (`.venv/bin/python`, `/usr/bin/python3`, `${workspace}/env/bin/python`) or another name
     (`python3.11`) stays exactly as written. Configurations suggested by plugins (SPEC-0276, run as given)
     are not changed. A `PATH` in the configuration's environment is used for the search.
   - The command line shown for a run (Run window, SPEC-0087 behaviour 13), a dry run of the MCP tool
     `run_configuration`, and the run configurations suggested by the MCP tool `project_model` name the
     interpreter actually used (`${workspace}/.venv/bin/python` inside the workspace, else its absolute
     path in the shown run).
   - When no interpreter is found at all (none chosen, no environment, nothing on `PATH` or in pyenv), the
     start does not happen and says why — status bar, Run window and the warning of the configuration list
     (SPEC-0087 behaviour 14), the error of `run_configuration`, the status of a debug start — never a silent
     fall-back to a `python3` that is not there. [IMP-0394]

## Acceptance criteria
- **AK-1** Layouts and selection: project environments (`.venv` with `version_info`, Windows layout),
  conda, pyenv, PATH with versions; order and reasons for configured, `VIRTUAL_ENV`, project,
  `.python-version`, conda.
- **AK-2** Packages from `dist-info` (with and without `METADATA`) and `egg-info`; specifier checks incl.
  `~=`, wildcards, Poetry `^`/`~`, pre/post releases; unsatisfied, conditional and skipped dependencies;
  `requires-python` mismatch; pytest configuration.
- **AK-3** A real environment made with `python3 -m venv`: the statically read version equals what the
  interpreter reports; `pip` installed, an undeclared package missing; the suggested unittest
  configuration runs a real test with the environment's interpreter.
- **AK-4** The MCP tool reports selection, compatibility, unsatisfied dependencies and test framework.
- **AK-5 (v2)** A workspace with `.venv/bin/python`: run current file, a test file, a test method, the
  suggested pytest configuration and a stored `run.toml` configuration starting with `python3` all run (and
  show) `.venv/bin/python`; a stored choice (SPEC-0117) wins over the `.venv`; `/usr/bin/python3` and
  `python3.11` stay as written; a plugin configuration is not changed; without any interpreter the start
  fails with the reason; the debug plan of a `python3` configuration names the selected interpreter;
  `project_model` and a dry run of `run_configuration` report it; in the window ⌃⇧R on a Python file starts
  the project's environment.

## Out of scope
Creating environments and installing packages (SPEC-0117), the debug adapter itself (SPEC-0091), the
pyright/basedpyright language server (SPEC-0170), Pipenv/Poetry/Hatch-managed environments outside the project,
evaluating environment markers, a `python3` inside a shell command line (`sh -c "python3 …"`), runs on a
remote host's own environment beyond what its host process finds there.

## Known deviations from the reference
Interpreters are never started for discovery (IMP-0025). v2: the reference, without an interpreter set up, creates
a run configuration that starts nothing and says nothing (OBS-0034, IMP-0394); IDECraft picks the project's
environment on its own (IMP-0396, IMP-0397), and a start without any interpreter names the reason.

v3 (OBS-0071): asked over MCP for the Python environment of a `.sql` file, the reference answers that
no interpreter is set up and proposes to create a new environment. `python_environments` reports the
workspace's interpreters and why one is selected, and never proposes or creates anything; creating a
virtual environment is a tool of its own with a dry run first, `python_create_venv` (IMP-0449).
