# SPEC-0001: Workspace

- **Version:** 2 · **Date:** 2026-10-31 (v2: settings directory renamed from `.ridelab` to `.idecraft`)
- **Sources:** GEN (common behaviour of editors and IDEs that open a folder as a project)
- **Feature matrix rows:** F-001

## Purpose
The user opens a directory as a workspace and sees its project files.

## Behaviour
1. A workspace is identified by its root directory; different spellings of the same directory denote the same workspace. [GEN]
2. Opening a path that is not an existing directory fails with a clear error. [GEN]
3. The file list contains regular files below the root, as paths relative to the root, sorted. [GEN]
4. Version-control metadata and common generated directories (`.git`, `.hg`, `.svn`, `target`, `node_modules`, `__pycache__`) and the IDE's own metadata directory are not listed. [GEN]
5. Symbolic links are not followed (prevents cycles). [GEN]
6. Workspace-specific settings live in `.idecraft/settings.toml` below the root and form the highest settings layer (SPEC-0002). A missing file is not an error. [GEN]

## Acceptance criteria
- **AK-1** Files are listed sorted and relative; excluded directories are absent.
- **AK-2** Opening a regular file or a missing path fails.
- **AK-3** Workspace settings override user settings.
- **AK-4** A workspace without settings file opens without error.

## Out of scope
`.gitignore` evaluation, file watching, multiple roots (later specs).
