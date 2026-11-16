# SPEC-0171: Searching the Python package index and installing from it

- **Version:** 2 · **Date:** 2026-11-10 (v2: the installer is also chosen from the environment and
  is one choice for the window, the import quick fix and MCP; an environment without pip and
  without uv is refused with the remedies — OBS-0034 L3)
- **Sources:** GEN (an IDE's package tool window searches the package index by name, shows a
  package's description and versions and installs the chosen version into the project's
  interpreter; F-035), STD (PyPI simple repository API in JSON, PEP 691, with the project list of
  `https://pypi.org/simple/`; the PyPI JSON API `https://pypi.org/pypi/<name>/json` with `info` and
  `releases`; name normalisation PEP 503; version ordering and pre-releases PEP 440; yanked files
  PEP 592; HTTP gzip content coding; `uv pip install --python`, `poetry add`,
  `python -m pip install`), SPEC-0050, SPEC-0117 (environment, pip installs, IMP-0042), SPEC-0053
  (agent `run` and `http` rights); v2: OBS-0034 (L3), STD (`uv venv` writes a `uv = <version>` line
  into `pyvenv.cfg` and installs no pip; `python -m ensurepip` adds pip to an environment),
  IMP-0025 (environments are read from files)
- **Feature matrix rows:** F-035

## Purpose
Find a package without knowing its exact name, pick a version and install it into the selected
environment with the tool the project uses, seeing the exact command first.

## Behaviour
1. **Name list:** the project names of the index come from the simple API (JSON, requested with
   gzip) and are kept as one name per line in the user cache folder
   (`…/idecraft/pypi-names.txt`). The list is downloaded on the first search and again when it is
   older than 7 days; while it is missing the search still finds an exact name (behaviour 3). Only
   `https://pypi.org` is contacted. [STD, GEN]
2. **Matching:** the query and the names are compared normalised (PEP 503, case-insensitive). Order:
   the exact name, then names starting with the query (shorter first), then names containing it
   (shorter first), at most 50. An empty query gives nothing. [GEN]
3. **Details:** for a chosen name the JSON API gives summary, latest version, `requires_python`,
   home page and the versions. Versions are sorted newest first by PEP 440 order; versions whose
   files are all yanked are left out; pre-releases are kept but marked. An unknown name (HTTP 404)
   is reported as not on the index. [STD]
4. **Installer (v2):** one choice, made in one place, for every install: the package chosen from
   the index here, the installs of SPEC-0117 behaviour 5 (the field, unmet declarations, "Install
   missing (n)", the import quick fix) and the MCP tools `python_package_info` and
   `python_install_packages`. In this order:
   1. **Project files:** `uv.lock` or `[tool.uv]` → uv; `poetry.lock` or `[tool.poetry]` → Poetry.
      Poetry is used only for adding a package from the index; the installs of SPEC-0117 never go
      through `poetry add`, which would rewrite the declaration they install, and continue with
      the next steps.
   2. **The environment (v2, OBS-0034 L3):** `pyvenv.cfg` of the selected environment has a
      `uv = …` line (it was made by `uv venv`) → uv; a site-packages folder of the environment
      exists and none holds a `pip` package folder → uv. Both are read from files; the interpreter
      is not started for the check (IMP-0025). Where no site-packages folder is found nothing is
      known about pip and it is taken as present.
   3. Else pip.

   The tool is looked up on `PATH`. Poetry not found → steps 2 and 3 decide, and the report says
   so. uv wanted but not found: when the environment has pip, pip is used and the report says why;
   when the environment has no pip, **nothing is started** and the message says that the
   environment has no pip and uv was not found, and names the two remedies: run
   `<interpreter> -m ensurepip` in that environment, or install uv. A choice of uv made by the
   environment is reported with its reason. Commands (no shell):
   - uv: `uv pip install --python <interpreter> <requirement>…`
   - Poetry: `poetry add <name>==<version>` (Poetry also records the dependency and installs into
     the environment it manages for the project)
   - pip: `<interpreter> -m pip install --disable-pip-version-check <requirement>…`
   From the index the requirement is `<name>==<version>`, or the bare name without a version
   (latest). [STD, OBS-0034]
5. As with SPEC-0117, uv and pip refuse a plain system or pyenv interpreter (IMP-0042); Poetry,
   which manages its own environment, is not refused. [GEN]
6. **Window:** in "Python Packages" the field searches as the user types (from 2 characters, after a
   short pause): matches replace the list, each marked when installed (with its version). Enter on a
   match loads its details: summary, latest, `requires-python`, the versions; ←/→ choose the
   version; the command that will run is shown before installing; Enter installs; Escape returns to
   the installed list. Typed requirements with operators or several words still install directly as
   in SPEC-0117. Results arrive in the background; the latest query wins. [GEN]
7. **MCP:** `python_search_packages` (`query`; names with installed versions) and
   `python_package_info` (`name`; details, the installer, the command for `version` or the latest;
   `install: true` runs it and needs the `run` right, logged like SPEC-0117). Both reach the
   index only with the `http` right. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Name list parsing from the simple API JSON (and gzip), matching order and limits, PEP 440
  ordering with pre-releases and yanked releases, installer choice and commands for uv/Poetry/pip,
  cache age (unit tests).
- **AK-2** Against the real index (skipped cleanly offline): `requests` details have a summary and
  versions with `2.31.0`; an invented name is reported as not on the index.
- **AK-3** A real uv (skipped without uv): the generated uv command, with `--offline --no-index
  --find-links <folder>` added by the test, installs a hand-made wheel from a local folder into a
  real venv; the environment then lists it with that version.
- **AK-4** MCP: search and details from a fake index, preview command, refusal without `http` and without `run`.
- **AK-5** Window self-test with a fake index: typing shows matches with the installed mark, Enter
  shows versions and the command, ← changes the version in the command, Escape returns.
- **AK-6** Hand test: search `rich`, install an older version, then the latest, next to the reference.
- **AK-7 (v2)** Fixture environments (with a `pip` folder, without one, with `uv = …` in
  `pyvenv.cfg`, without a site-packages folder) and `uv` found or not through an injected `PATH`:
  pip with pip present; uv with every requirement and `--python <interpreter>` without pip or for
  an environment made by uv; pip with the reason when uv is missing but pip is there; the refusal
  naming `-m ensurepip` and installing uv when both are missing, with no command; the same choice
  for the index install; never Poetry for the installs of SPEC-0117; the shared-interpreter refusal
  unchanged (unit tests).
- **AK-8 (v2)** MCP: the preview of `python_install_packages` for an environment without pip shows
  the uv command and `installer: "uv"` with uv on the injected `PATH`, is refused with the remedies
  without it, and shows the pip command and `installer: "pip"` for an environment with pip.

## Out of scope
Searching descriptions or keywords (the index offers no search API), other indexes and
authentication, uninstalling and upgrading, Pipenv/Hatch/PDM/conda as installers, `uv add`,
running `ensurepip` or installing uv for the user, looking for uv outside `PATH`.

## Known deviations from the reference
Search matches names only. The install command is always shown before it runs. Not compared side by
side yet.
