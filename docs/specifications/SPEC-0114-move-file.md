# SPEC-0114: Move a file

- **Version:** 5 · **Date:** 2026-11-18 (v5: Java and Kotlin references follow a moved type also
  where the language server gives no edits, behaviour 9; a move is one undo group, behaviour 10;
  window check A12 V1, V2); 2026-11-18 (v4: with the Project tree focused, F6 moves the
  tree's selection, also a folder, and the move can be undone from the tree, SPEC-0275 v22
  behaviour 49; OBS-0088 U1); 2026-11-13 (v3: only the project's own sources are changed, behaviour 8;
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
   (v4) With the Project tree focused, `Move` acts on the tree's selected file or folder instead
   of the active file, and the move is a step of the tree's history of file operations
   (SPEC-0275 v22 behaviour 49). [OBS-0088]
2. **Target:** the file keeps its name; the folder is created when missing. A folder outside the
   workspace, the file's own folder and a target that exists are refused. [GEN]
3. **References:** the file's language server is asked for its edits for the move
   (`workspace/willRenameFiles`, announced as a client capability) — Rust modules, Java packages,
   TypeScript imports where the server offers them. Ferris IDEA rewrites itself what servers leave:
   relative `#include "…"` in C and C++ and relative import specifiers in JavaScript and TypeScript
   (`import … from`, `export … from`, `require(…)`, `import(…)`), keeping or dropping the extension
   as written; a specifier the server already rewrote is not changed twice. Inside the moved file its
   own relative references are rewritten to start from the new folder. [STD, GEN]
4. **Applying:** open files other than the moved one change through their editors (one undo step
   each); the moved file and closed files are written on disk, then the file is moved. The moved
   file's tab closes and opens at the new place; the project tree is read again and keeps its
   expanded folders. Since v5 the move is one undo group instead (behaviour 10). [GEN, SPEC-0074]
5. **Report:** the status bar names the new place and how many files were updated, and whether the
   language server took part. [GEN]
6. **MCP:** `move_file` (path, target_dir, `dry_run` default true) returns the target and the edits,
   and with `dry_run=false` writes them and moves the file. Folders, a new name and a plain move
   without reference updates (`update_references: false`) follow SPEC-0275 v2 behaviour 23.
   [ADR-0008]
7. **Rust modules (v2, D27):** rust-analyzer gives no edits for a move into another folder, so
   `mod geld;` no longer found `src/finanz/geld.rs` and the crate stopped building. Ferris IDEA keeps
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
   the language server were applied wherever they pointed, and Ferris IDEA's own rewriting of
   includes and imports went over every file of the workspace except a short list of folder
   names, so a move could rewrite files of an installed package. [GEN]
   - **Taken as named:** the moved file itself and the target folder the user names are not
     judged by the rule: a file in `.venv` can be moved, its own relative references follow, and
     a target folder in `node_modules` is used.
   - **Listed apart, not applied:** every other file with edits (from the language server or
     from Ferris IDEA's own rewriting) that is not the project's own is left out of the plan and
     listed with its edits and the reason. Folders Ferris IDEA's own rewriting never read
     (`node_modules`, `target`, `__pycache__`, version control and Ferris IDEA data) are still not
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

9. **Java and Kotlin references (v5, window check A12 V1):** where the language server gives no
   edits for a file (no server, no answer in time, or a file it has not indexed, e.g. one another
   program created a minute before), Ferris IDEA rewrites the references to the moved type itself,
   reading every Java and Kotlin file of the workspace from the disk (or the editor's unsaved text):
   - every qualified name `<old package>.<Type>` standing alone — `import`, `import static
     ….<Type>.member`, qualified uses in code — becomes `<new package>.<Type>`; a longer name
     (`….InventoryItem`, `x.lab.inventory.Inventory`) is left alone;
   - a file of the old package that uses the type by its simple name, or a file that has
     `import <old package>.*` and uses it, gets `import <new package>.<Type>` after its last
     import (or after its package line, with a blank line); a file of the new package and one that
     already imports a type of that name get nothing;
   - the moved file gets imports for the types of its old package it uses by their simple names
     (the names of the other files in its old folder);
   - files the server changed are left to the server; an own edit overlapping a server's is
     dropped. The old and new package come from the moved file's package line and the target
     folder (SPEC-0275 v3 behaviour 26); without a package line, or for a move into the default
     package, nothing is rewritten. Kotlin imports have no `;`. Before v5 a Java class moved with
     F6 got its new package line, but `import lab.inventory.Inventory;` in another file stayed
     and the build broke. [GEN]
10. **One undo group (v5, window check A12 V2):** a move with F6 is one refactoring group of
    SPEC-0193 named "Move <file> to <folder>" (v7 behaviour 12), replacing the "one undo step
    each" of behaviour 4: open files, also the moved file, change through their editors; the
    others are written on disk; then the file moves (its editor, tab, bookmarks and language
    servers follow). ⌘Z in any of its editors, or in the Project tree while no file operation came
    after it, moves the file back and restores its text and every updated reference, with the
    checks and refusals of SPEC-0193 behaviour 3; the status bar says "Undone: Move <file> to
    <folder> (n files)". ⇧⌘Z moves it again. The target folder made by the move stays. The
    MCP tool `move_file` (`dry_run=false`, one `path`) keeps the move as a group of
    `undo_refactoring` and `redo_refactoring` (SPEC-0193 behaviour 7). [GEN, ADR-0008]

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

- **AK-9 (v5)** Java references (unit tests `move_file::tests::java_references_follow_the_moved_type`,
  `move_file::tests::the_moved_type_imports_its_old_neighbours`); in a real folder, with or
  without jdtls, `Inventory.java` moved from `lab.inventory` to `lab.stock`: its package line,
  the import of `Main.java` and an import in its old neighbour follow, the moved file imports
  the neighbour (`crates/languages/tests/it/move_file.rs::moving_a_java_class_updates_its_imports`).
- **AK-10 (v5)** In a window of the test harness: F6 on the open `Inventory.java`, target
  `src/main/java/lab/stock`, then ⌘Z in its editor: file back, package line and the import of
  `Main.java` restored, "Undone: Move Inventory.java to src/main/java/lab/stock (2 files)"; ⇧⌘Z
  moves it again; ⌘Z in the Project tree undoes it too; after `Main.java` changed on disk the
  undo is refused and nothing moves (`crates/workbench/tests/it/move_undo_a12.rs`). MCP:
  `move_file` then `undo_refactoring` moves the file back and restores the reference
  (`crates/mcp/tests/it/protocol.rs::move_file_is_undone_by_undo_refactoring`).

## Out of scope
Moving folders, several files at once, moving symbols (a class or function out of a file), renaming
a file (Rename on a file), drag and drop in the tree, include paths resolved through `-I` folders or
`tsconfig` path aliases, Python and Go import rewriting, Ferris IDEA's own rewriting of Kotlin
top-level functions and type aliases, and of a moved type's nested or non-public neighbours
(behaviour 9 knows only the types named like their files).

## Known deviations from the reference
The target is typed as a folder path instead of chosen in a package dialog. For Java, Rust and
TypeScript the result depends on what the language server offers for `willRenameFiles`. A Rust module
moved into another folder keeps its place in the module tree through `#[path]` (behaviour 7) instead of
becoming a child module of the target folder's module. Not compared side by side yet.
Since v3 references outside the project's own sources are listed and left unchanged (behaviour 8);
the reference was not compared for this. The window has no preview with rows to tick for them.
