# SPEC-0081: Go to Implementation and Go to Type Declaration

- **Version:** 4 · **Date:** 2026-11-13 (v4: Quick Definition shows the source of the implementation in the documentation popup, behaviours 6 and 7, AK-6 and AK-7; docs/research/KEYMAP_COVERAGE.md; v3 2026-11-07; v2: chooser rows name the class, its package and module, OBS-0016 defect F034-1;
  v3: a Java row without symbols gets its type from the syntax tree, OBS-0027)
- **Sources:** GEN (jump to the implementations of a method or to the declaration of a symbol's type,
  with a chooser when there are several; F-034), OSS (action ids `GotoImplementation` ⌥⌘B / Ctrl+Alt+B
  and `GotoTypeDeclaration` ⇧⌘B / Ctrl+Shift+B in the ported keymaps), STD (LSP
  `textDocument/implementation` and `textDocument/typeDefinition`), SPEC-0071 (Go to Declaration, Back
  and Forward), SPEC-0073 (the usages popup)
- **Feature matrix rows:** F-034

## Purpose
Get from an interface method to the code that runs, or from a variable to the type it holds.

## Behaviour
1. **Start:** `GotoImplementation` asks the active file's language server for the implementations and
   overrides of the symbol at the caret, `GotoTypeDeclaration` for the declaration of its type; the
   server receives the editor's current text. The status bar says what is looked for. [GEN, OSS, STD]
2. **Result:** the caret's own place is skipped. One target opens in a tab with the caret at its start
   and the place left is recorded for Back. Several targets are listed in a popup titled "Choose
   implementation (n)" (or "type declaration", "declaration") with file icon, line text, file and line;
   Enter or a click jumps, Escape closes. Go to Declaration uses the same chooser (SPEC-0071 v3). Without
   a result the status bar says "Cannot find a … to go to." [GEN]
3. **MCP:** `find_symbol_locations` takes the kinds `type_definition` and `implementation` besides
   `definition` and `references`. [ADR-0008]
4. **Rows name the class (v2):** before the chooser opens, the server is asked for the symbols of
   each target's file (at most 5 s in all). A row then shows the symbol icon of the innermost type
   that contains the target, that type's name with its enclosing types joined by `.`
   (`Discounts.Half`), the package in grey after it (the file's package symbol; else its
   namespaces; else the folder below the module) and the module at the right edge (the nearest
   folder up to the project root that holds a build file — `pom.xml`, `build.gradle(.kts)`,
   `settings.gradle(.kts)`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, `setup.py`,
   `CMakeLists.txt` — else the project folder; its folder name). Rows are sorted by that name,
   then package, module, path and line. The title names the symbol as the server names it at the
   first target, with its parameter types when the server gives them: `Choose implementation of
   apply(int) (2)`. A target outside every type (a C function), or a file whose symbols do not
   arrive in time, keeps the row of behaviour 2. The reference names class, outer class, package
   and module per row, sorted alphabetically; IDECraft showed two equal code lines (OBS-0016 Nr. 5).
   [OBS-0016, STD, GEN]
   4a. **Java without symbols (v3):** when the symbols of a Java target's file do not arrive in time,
   the row takes the innermost type around the target and the file's `package` line from the syntax
   tree, so all rows of a chooser look alike (the reference shows `Cat (shop)`, `Dog (shop)`).
   [OBS-0027 L14]
5. **MCP (v2):** `find_symbol_locations` adds to each location what is known of it: `container`
   (`Discounts.Half`), `package`, `module` and `symbol` (the server's name of the symbol there).
   [ADR-0008, IMP-0301]
6. **Quick Definition (v4):** `QuickImplementations` (⌥Space and ⌘Y on macOS, Ctrl+Shift+I elsewhere) shows the source of the symbol at the caret without leaving the editor. The language server is asked for the implementations as in behaviour 1 (the editor's unsaved text counts); when it names none, or does not answer that request, it is asked for the definition. The answer is shown in the popup of Quick Documentation (SPEC-0077), below the caret and closed the same way; nothing in it can be edited and no tab is opened. For each target, at most five in the order of path and line, the popup shows a grey line with the file's path relative to the project and the 1-based line, then the source: the innermost declaration of the file's syntax tree (class, interface, method or function, with its annotations and modifiers) that starts at or encloses the target's line, cut after 40 lines with a line `…`; when the syntax tree names no declaration there, the 12 lines from the target's line on. Targets are separated by a rule; more than five end with a line that says how many are not shown and that Go to Implementation(s) lists all. A target outside the project (a library class) has no source here; when there is no other target the status line says so. Without any target the status line says so. For a document of a remote project the action says that it is not available. [OSS: id and shortcuts of the bundled keymaps; STD: `textDocument/implementation`, `textDocument/definition`; GEN]
7. **MCP (v4):** `quick_definition` (path, line, character) returns `kind` (`implementation` or `definition`, whichever gave the targets) and `targets`, each with `path` (relative to the workspace when inside it), `line` and `end_line` (0-based, the shown lines), `text` and `truncated`, and `outside`, the names of targets that are no files (library classes); all targets are returned, each cut as in behaviour 6. The saved content is asked about. Reads only. [ADR-0008]

## Acceptance criteria
- **AK-1** With clangd on unsaved C++ text: the type declaration of a variable, and both overrides of a
  virtual method (integration test).
- **AK-2** In a real window (`idecraft ui --selftest`, with clangd): after typing
  `typedef int num; num g;`, ⇧⌘B on `g` jumps to the typedef.
- **AK-3** Use by hand: ⌥⌘B with several implementations (chooser), Back after the jump, ⌘B with
  several definitions.
- **AK-4 (v2)** The class path, package, module and order of chooser rows (unit tests
  `context::tests::the_innermost_type_names_the_place`, `context::tests::the_module_is_the_nearest_build_folder`,
  `usages_model::tests::chooser_rows_name_the_class`); with jdtls, the implementations of
  `Discount.apply` are `Discounts.Half` and `Discounts.None` in `lab.inventory` (live test
  `tests/java_navigation.rs`).
- **AK-5 (v3)** A Java place without symbols gets type, kind and package from the syntax tree
  (`context::tests::java_owner_from_the_syntax_tree`).
- **AK-6 (v4, behaviour 6)** The source of a target: the innermost declaration at or around the line, the cut at 40 lines, the 12 lines without a declaration, a line outside the file (unit tests `quick_definition::tests::the_declaration_at_or_around_the_line`, `quick_definition::tests::long_declarations_and_plain_lines`); the popup's blocks for one and for seven targets (`keymap_actions::tests::quick_definition_blocks_name_each_target`); Find Action titles the action Quick Definition under View (`everywhere::tests::navigation_actions_from_the_keymap_have_titles`). In the window with a language server: open.
- **AK-7 (v4, behaviour 7)** MCP `quick_definition` is listed with its schema and refuses a path outside the workspace (protocol test `navigation_tools_from_the_keymap_coverage`); with a language server: open.

## Out of scope
Implementations searched by IDECraft itself (without a server), editing or navigating inside the
Quick Definition popup and stepping through its targets with keys (v4 shows them one below the other),
Go to Super Method (⌘U), gutter icons for implemented and overriding methods.

## Known deviations from the reference
The title shows the parameter types only when the language server includes them in its symbol
names (jdtls does, clangd does not). What is found depends on the language server; for C, implementations exist only in C++ with virtual
methods.
