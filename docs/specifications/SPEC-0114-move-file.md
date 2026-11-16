# SPEC-0114: Move a file

- **Version:** 3 · **Date:** 2026-11-13 (v3: only the project's own sources are changed, behaviour 8;
  v2: Rust module declarations follow a move into another folder; hand test Nr. 41, D27)
- **Sources:** GEN (Move puts a file or class into another folder or package and updates what refers
  to it; F-072), OSS (action id `Move`, F6 in the default keymap), STD (LSP
  `workspace/willRenameFiles` and the client capability `workspace.fileOperations`; C/C++
  `#include "…"` resolved relative to the including file; ECMAScript relative module specifiers),
  SPEC-0040 (edits per file), SPEC-0074 (applying edits), SPEC-0059 (tabs and tree), ADR-0008
- **Feature matrix rows:** F-072

## Purpose
Reorganise files without breaking the code that uses them.

## Behaviour
1. **Opening:** `Move` (F6) on the active file opens a field with its folder relative to the
   workspace, selected; the user types the target folder. Enter moves; Escape closes. A file with
   unsaved changes is not moved (the status bar asks to save it first). [GEN, OSS]
2. **Target:** the file keeps its name; the folder is created when missing. A folder outside the
   workspace, the file's own folder and a target that exists are refused. [GEN]
3. **References:** the file's language server is asked for its edits for the move
   (`workspace/willRenameFiles`, announced as a client capability) — Rust modules, Java packages,
   TypeScript imports where the server offers them. IDECraft rewrites itself what servers leave:
   relative `#include "…"` in C and C++ and relative import specifiers in JavaScript and TypeScript
   (`import … from`, `export … from`, `require(…)`, `import(…)`), keeping or dropping the extension
   as written; a specifier the server already rewrote is not changed twice. Inside the moved file its
   own relative references are rewritten to start from the new folder. [STD, GEN]
4. **Applying:** open files other than the moved one change through their editors (one undo step
   each); the moved file and closed files are written on disk, then the file is moved. The moved
   file's tab closes and opens at the new place; the project tree is read again and keeps its
   expanded folders. [GEN, SPEC-0074]
5. **Report:** the status bar names the new place and how many files were updated, and whether the
   language server took part. [GEN]
6. **MCP:** `move_file` (path, target_dir, `dry_run` default true) returns the target and the edits,
   and with `dry_run=false` writes them and moves the file. Folders, a new name and a plain move
   without reference updates (`update_references: false`) follow SPEC-0275 v2 behaviour 23.
   [ADR-0008]
7. **Rust modules (v2, D27):** rust-analyzer gives no edits for a move into another folder, so
   `mod geld;` no longer found `src/finanz/geld.rs` and the crate stopped building. IDECraft keeps
   the module where it is in the module tree and points its declaration at the new file: a
   `#[path = "finanz/geld.rs"]` line is put before `mod geld;` in the file that declares it (the
   crate root or `mod.rs` of the old folder, or the folder's `name.rs`; the path is relative to the
   folder Rust resolves it from). A further move replaces that line, a move back to where the module
   tree expects the file removes it. Paths such as `crate::geld::runde` stay valid. [GEN]
8. **Only the project's own sources (v3; decision of the coordinator of 2026-11-13 under the Project Owner's general authority: the rule the Project Owner chose for Rename, Find in Files, Search Everywhere, Inspect Code and Endpoints):** a move changes references only in files that
   are the project's own by the rule of SPEC-0148 v4 behaviour 7 (`OwnSources`): not in a virtual
   environment, installed dependencies, build output and caches, version control and IDE data,
   folders the project model excludes, anything behind a symbolic link or outside the project
   folder, anything the ignore rules of Git match unless Git tracks it. Before v3 the edits of
   the language server were applied wherever they pointed, and IDECraft's own rewriting of
   includes and imports went over every file of the workspace except a short list of folder
   names, so a move could rewrite files of an installed package. [GEN]
   - **Taken as named:** the moved file itself and the target folder the user names are not
     judged by the rule: a file in `.venv` can be moved, its own relative references follow, and
     a target folder in `node_modules` is used.
   - **Listed apart, not applied:** every other file with edits (from the language server or
     from IDECraft's own rewriting) that is not the project's own is left out of the plan and
     listed with its edits and the reason. Folders IDECraft's own rewriting never read
     (`node_modules`, `target`, `__pycache__`, version control and IDECraft data) are still not
     read, so nothing is listed from them unless the language server names a file there.
   - **MCP:** `move_file` returns these files as `outside_project` (numbered `index`, `path`,
     `why`, `reason`, `edits`, `included`) next to `plan`; `include_outside: [indices]` changes
     the given ones too, an unknown index is refused and nothing is moved. With `paths` each
     result carries its own `outside_project`, and `include_outside` is refused, because the
     indexes belong to one plan.
   - **Window:** Move has no preview (behaviour 4 applies at once), so the files left out are
     only counted: the status bar adds "N file(s) outside the project's own sources left
     unchanged." They can be changed on purpose through `move_file`.
   - When it is unsure whether a file is the project's own, it is not changed.

## Acceptance criteria
- **AK-1** Paths normalise and relate; targets outside the workspace and in place are refused;
  includes and imports (with and without extension, `require`, `export from`, packages left alone) are
  rewritten for the users and inside the moved file (unit tests `move_file::tests`).
- **AK-2** In a real folder: a header and a TypeScript module are moved; users and the moved files'
  own references follow (`crates/languages/tests/it/move_file.rs`).
- **AK-3** MCP: preview, then move a header; the include follows (`crates/mcp/tests/it/protocol.rs`).
- **AK-4** Window self-test: F6 offers the file's folder; moving into a subfolder rewrites the
  include of the user, and the tab shows the moved file.
- **AK-6 (v2)** A Rust module moved into `src/finanz` and back: `cargo check` passes after each
  move, the `#[path]` line comes and goes (`tests/move_file.rs::moving_a_rust_module_keeps_the_crate_building`);
  inserting, replacing and removing the line, the non-`mod.rs` layout and declarations left alone
  (unit test `move_file::tests::rust_modules_follow_the_moved_file`).
- **AK-5** Hand test (round 4): moving a Java class to another package and a Rust module file, next
  to the reference.
- **AK-7 (v3)** In a temporary project with `.venv/lib/python3.12/site-packages/x.c`,
  `node_modules/y.c`, a file an ignore rule matches, one Git tracks although the rule matches it
  and own files, all including the moved header: the plan holds the own files and the tracked one;
  the environment's file and the ignored one are listed apart with the reason and stay unchanged;
  an entry included on purpose changes, an unknown index is refused; a file moved out of `.venv`
  into `node_modules/z` is moved and its own reference follows
  (`crates/languages/tests/it/move_file.rs::a_move_changes_only_the_projects_own_files`); the
  status line counts the files left out (`usages_model::tests::move_report_names_the_target`).
- **AK-8 (v3)** MCP on the same project: `plan` and `outside_project` (index, why, reason, edits,
  `included`), `include_outside`, the refusals for an unknown index and for `paths`
  (`crates/mcp/tests/it/protocol.rs::move_file_leaves_files_outside_the_own_sources_alone`).

## Out of scope
Moving folders, several files at once, moving symbols (a class or function out of a file), renaming
a file (Rename on a file), drag and drop in the tree, include paths resolved through `-I` folders or
`tsconfig` path aliases, Python and Go import rewriting.

## Known deviations from the reference
The target is typed as a folder path instead of chosen in a package dialog. For Java, Rust and
TypeScript the result depends on what the language server offers for `willRenameFiles`. A Rust module
moved into another folder keeps its place in the module tree through `#[path]` (behaviour 7) instead of
becoming a child module of the target folder's module. Not compared side by side yet.
Since v3 references outside the project's own sources are listed and left unchanged (behaviour 8);
the reference was not compared for this. The window has no preview with rows to tick for them.
