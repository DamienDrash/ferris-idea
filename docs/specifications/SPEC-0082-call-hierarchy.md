# SPEC-0082: Call hierarchy

- **Version:** 8 · **Date:** 2026-11-19 (v8: Java callees include `new` of a record, behaviour 19, AK-15,
  OBS-0071 O4); 2026-11-16 (v7: paths of the MCP hierarchy tools are workspace-relative, behaviour 18, AK-14, OBS-0071 G2, IMP-0445); 2026-11-11 (v1: domain function and MCP tool; v2: Hierarchy tool window; v3: closing it, hand test; v4: on the right, all levels open, rows with class, sorting, OBS-0016 defect F034-7; v5: type hierarchy,
  method hierarchy and Go to Super Method for Java, OBS-0027; v6: the same three for Kotlin, OBS-0035 L5)
- **Sources:** GEN (who calls a function and what it calls, as an expandable tree; F-034), OSS (action
  id `CallHierarchy` with ⌃⌥H in the ported keymaps), STD (LSP `textDocument/prepareCallHierarchy`,
  `callHierarchy/incomingCalls`, `callHierarchy/outgoingCalls`), SPEC-0072 (requests on unsaved text)
- **Feature matrix rows:** F-034, F-037 (v5)
- **Script files (2026-11-09):** see SPEC-0320, which extends this spec for JavaScript and TypeScript.

## Purpose
Follow how control reaches a function, or where it goes from there, without reading every file.

## Behaviour
1. **Domain:** `call_hierarchy_in` asks the language server for the function at a position, on the
   editor's text when given, and expands its callers or its callees to a requested depth. Each node has
   name, kind, detail, the place of its name, and the call sites between it and its parent; call sites
   are always places in the calling function's file. Without a function at the position the result is
   empty. [STD]
2. **Limits:** a node that repeats one of its ancestors (direct or indirect recursion) is marked as
   recursive and not expanded again; at most 500 nodes are collected per hierarchy, so a deep or
   wide tree cannot keep the server busy without end. [GEN]
3. **MCP:** `call_hierarchy` takes path, line, character, `direction` (`callers` | `callees`) and
   `depth` (1–10, default 3) and returns the tree for the saved file; an unknown direction is
   refused. [ADR-0008]

4. **Start (v2):** `CallHierarchy` (⌃⌥H) asks for the callers of the function at the caret on the
   editor's text, five levels deep, and opens the Hierarchy tool window at the bottom of the editor
   area; the status bar says it is building, that there is no function at the caret, or why it failed.
   [GEN, OSS]
5. **Tool window:** a header "Hierarchy · Callers of <name>" with the buttons Callers and Callees; the
   other button asks again for the same function in that direction. The tree starts with the root
   expanded; each row has a chevron when it has children, the symbol icon, the name, "(recursive)" for a
   repeated ancestor, and file:line with the number of calls, dimmed. The selection covers the whole
   row. [GEN]
6. **Keys and mouse:** Up/Down select, Right expands or moves to the first child, Left collapses or
   moves to the parent, Enter or a double-click jumps to the function (recording the place left for
   Back) and keeps the tool window open, Escape closes it and returns to the editor. [GEN]
7. **Close (v3):** a × in the header closes the tool window; `HideActiveWindow` (⇧Esc) closes it from the
   editor too (and otherwise hides Find in Files, then the terminal). [GEN, OSS]

8. **On the right, all open (v4):** the Hierarchy tool window docks at the right edge of the
   window, beside the editor and the right tool windows, 420 px wide and as high as the editor
   area, instead of at the bottom (this replaces "at the bottom" in behaviour 4). Every node with
   children starts expanded (this replaces "the root expanded" in behaviour 5). The header reads
   `Callers of <name>` / `Callees of <name>`. [OBS-0016 Nr. 10]
9. **Rows (v4):** a row shows the function's name as the server gives it without a trailing
   return type (`priceOf(Item, Discount)` from `priceOf(Item, Discount) : int`), then in grey the
   server's detail in parentheses (`(lab.inventory.Discounts)`), or `file:line` when there is no
   detail, and `(n calls)` when the parent is called from more than one place. [OBS-0016 Nr. 10]
10. **Toolbar (v4):** Callers, Callees, Sort Alphabetically (children of every node by name; off:
   the server's order), Expand All, Collapse All (the root's children stay visible), Refresh (asks
   again for the same function and direction), Close. [OBS-0016 Nr. 10, GEN]

11. **Java class structure (v5):** for Java the same tool window shows type hierarchies and method
    hierarchies, built from the language server's type hierarchy (STD: LSP 3.17
    `textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes`) and,
    for the methods, the syntax tree of each project file (open editors' unsaved text first).
    [STD, GEN]
12. **Header (v5):** the heading gives way to the tabs and buttons when the window is narrow (it is
    cut with "…"), so Close always stays visible. [OBS-0027]
13. **Type Hierarchy (v5):** `TypeHierarchy` (⌃H) on a type, or anywhere inside one, opens `Class
    <name>`: the superclass chain from the topmost class (`Object`) down to the type, marked `*`, with
    all its subtypes below; the tabs Class, Supertypes (the type at the root, its superclass and
    interfaces above in the tree's children) and Subtypes switch the part. Rows show the type icon,
    the name and the package in grey. [GEN, OBS-0027 L4]
14. **Method Hierarchy (v5):** `MethodHierarchy` (⇧⌘H) on a method opens `Method <name>`: the types
    from the topmost project supertype that declares the method down through all subtypes, each
    marked `+` (defines it), `-` (inherits it) or `!` (a concrete type that must define it because no
    type above it in the tree has a body), with a legend of the marks under the tree. [GEN,
    OBS-0027 L5]
15. **Go to Super Method (v5):** `GotoSuperMethod` (⌘U) in a method jumps to the method it overrides
    or implements, searched breadth first through the project's supertypes (a match by name and
    parameter types, generic arguments and packages ignored), recording the place left for Back; on
    a type outside a method it jumps to its superclass. A target in a library type is named in the
    status bar (library sources cannot be opened yet); nothing above says so. Navigate menu: Super
    Method, Type Hierarchy, Method Hierarchy. [GEN, OBS-0027 L3]
16. **MCP (v5):** `type_hierarchy` (`direction` class | supertypes | subtypes), `method_hierarchy`
    and `super_method` take path, line and character of the saved file. [ADR-0008]
17. **Kotlin (v6):** behaviours 13–16 work in `.kt` files, with the same actions, keys, tool window
    and MCP tools. kotlin-language-server 1.3.12 answers `textDocument/prepareTypeHierarchy` and
    `textDocument/implementation` with an error (tried on the project of OBS-0035), so the answer
    comes from the syntax trees of the project's `.kt` files (hidden folders and `build`, `out`,
    `target`, `bin`, `node_modules` left out; at most 5 000 files of at most 1 MB; open editors as
    they are, unsaved). No language server is needed, also not in safe mode. [OBS-0035 item 9, STD,
    GEN]
    - *Types:* classes (also `data`, `sealed`, `abstract`, `enum`), interfaces, objects and
      companion objects, nested ones included. A name in a supertype list means a project type when
      a type of that name is declared in the same file, in the same package, or is imported (by
      name, through its outer type, or with `*`), or when it is written with its full name; anything
      else is a type outside the project.
    - *Type hierarchy:* as behaviour 13. The class chain follows the supertype that is a class (a
      project class, or a name written as a constructor call); a chain that ends at a project class
      gets `Any` (`kotlin`) as its root, an interface has no class above it. A type outside the
      project is a leaf without subtypes; activating it leads to where it is named in the supertype
      list, and `Any` to the topmost project class.
    - *Method hierarchy:* as behaviour 14, for a function or a property (also a `val`/`var` of the
      primary constructor): from the farthest project supertype that declares it. Members match by
      kind, name and number of parameters. `!` marks a class or object that is neither `abstract`
      nor `sealed`.
    - *Go to Super Method:* as behaviour 15, for functions and properties; on a type outside a
      member its superclass, else its first supertype. A supertype outside the project is named in
      the status bar. A member that overrides one of a library type finds nothing.
18. **Paths over MCP (v7):** `call_hierarchy`, `type_hierarchy`, `method_hierarchy` and
    `super_method` give every `location.path` inside the workspace relative to the workspace root
    with `/`, as `find_symbol_locations` does; a place outside the workspace (a library, the JDK) keeps
    its absolute path, and a place the server gives no file for keeps an empty path. The tool window
    is not affected: it works on the domain result. [GEN; OBS-0071 G2, IMP-0445]
19. **Java constructor calls (v8):** jdtls leaves a `new R(…)` out of a method's outgoing calls when
    `R`'s constructor is not written in the source (a record's canonical constructor; a class without
    constructors and an anonymous class it does report, as the type). For the callees of a Java
    method or constructor, every `new` expression of its body (not those inside the body of an
    anonymous or local class, which belong to that class's members) that has no call site of exactly
    its range among the server's answer is added: the server's call hierarchy item at the created
    type's simple name (for a record the type, as jdtls shows a class without constructors), with
    the expressions as call sites, one node per type, placed in source order among the server's
    callees, expanded like any other node. Nothing is added for callers, other languages, or when the
    server finds no item at the name. Read from the syntax tree of the method's file (the editor's
    text for the file asked about). [GEN, STD; OBS-0071 O4]

## Acceptance criteria
- **AK-1** With clangd on unsaved C: callers of `leaf` are `mid` (two call sites on its line) and `rec`;
  `mid`'s caller is `top`; `rec` calling itself is marked recursive and not expanded; callees of `top`
  are `mid` and then `leaf`, with the call site in `mid` (integration test).
- **AK-2** MCP `call_hierarchy` returns callers and refuses an unknown direction (protocol test).
- **AK-3** Rows follow expand and collapse (unit test of `hierarchy_model`).
- **AK-4** In a real window (`ferris-idea ui --selftest`, with clangd): ⌃⌥H on `calc` opens the tool window
  with `calc` as root and callers; Callees asks again; Escape closes it.
- **AK-5** Use by hand: deeper trees, recursion, jumping, the two buttons.
- **AK-6 (v4)** All levels open, sorting, collapse all and the row label (unit tests
  `hierarchy_model::tests::every_level_starts_open`, `hierarchy_model::tests::sorted_by_name`,
  `hierarchy_model::tests::row_label_drops_the_return_type`); in a real window ⌃⌥H shows the tree
  docked at the right with every level open (window self-test).
- **AK-9 (v5)** With jdtls: class hierarchy Object › * Animal › Cat, Dog from inside Animal's body,
  supertypes of Dog, the super methods of Dog.sound() and Dog.owner() and the superclass of Dog, the
  method hierarchy of Cat.sound() with `+` on all three (`crates/languages/tests/it/java_structure.rs`);
  marks `+`, `-`, `!` across five classes (`java_structure::tests::method_marks_follow_the_types`).
- **AK-10 (v5)** Window self-test `java_structure_steps`: ⌃H shows the class hierarchy, Escape closes
  it, ⌘U in Dog.sound() lands on Animal.sound(), ⇧⌘H marks `+ Animal`, `+ Dog`, `! Cat`.
- **AK-11 (v5)** MCP tool list names `type_hierarchy`, `method_hierarchy`, `super_method`
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-12 (v6)** On the Kotlin shapes of OBS-0035 (abstract `Animal` with `sound()`, interface `Pet`
  with `val owner: String`, `Dog`, `Cat`, sealed `Shape`) and a second package: declarations,
  modifiers and supertypes are read; the class hierarchy of `Animal` is `Any › * Animal › Dog, Cat,
  Cow › Calf`, of sealed `Shape` its two data classes; supertypes of a class with a library
  interface show it as a leaf placed where it is named; a type of another package without import
  is not the project's type; the method hierarchy of `Pet.owner` marks `+ Pet`, `! Dog`, `+ Cat`;
  the super member of `Cat.sound()` is `Animal.sound()`, of `Cat`'s `owner` `Pet.owner`, of `Dog`
  `Animal` (unit tests `kotlin_structure::tests`); the same through the entry points on files on
  disk, with a copy in `build/` left out and the editor's unsaved text winning
  (`crates/languages/tests/it/kotlin_structure.rs`); MCP `type_hierarchy`, `method_hierarchy` and
  `super_method` on a `.kt` file (`crates/mcp/tests/it/protocol.rs`).
- **AK-13 (v6)** In a window (`crates/workbench/tests/it/kotlin_structure.rs`): `TypeHierarchy` (bound
  to ⌃H) on `Animal` in a Kotlin file fills the Hierarchy tool window with `Any`, `* Animal`, `Dog`,
  `Cat`; `MethodHierarchy` on `Pet.owner` shows `+ Pet`, `! Dog`, `+ Cat`; `GotoSuperMethod` in
  `Cat.sound()` puts the caret on `Animal.sound()`; in a text file the status bar says the action
  works in Java and Kotlin files.
- **AK-14 (v7)** MCP: in the Kotlin structure project, `type_hierarchy`, `method_hierarchy` and
  `super_method` give `src/main/kotlin/zoo/Animals.kt` as the path of their places
  (`protocol.rs::kotlin_structure_tools`); with clangd, `call_hierarchy` gives `src/calc.c` for the
  function and its caller (`protocol.rs::code_insight_tools`); the rewriting keeps absolute paths
  outside the root and empty paths (unit test `tests::locations_inside_the_workspace_become_relative`
  in `crates/mcp/src/lib.rs`).

- **AK-15 (v8)** The `new` expressions of a method body with their range and the start of the type's
  simple name (plain, qualified, generic, nested type), none from an anonymous class's body, none for a
  type's or a field's name (unit test `java_calls::tests::creations_of_a_method_body`); with jdtls the
  callees of `main` have `Rec` once, at the record's name, with its three call sites in `main` in
  order (one inside `List.of(…)`, one followed by `.toString()`) and none from the anonymous class,
  after `println` and before `Runnable`, and `Item`, `Runnable`, `Shop`, `totalValue`, `println` once
  each (`crates/languages/tests/java_method_refs.rs`).

## Out of scope
Loading levels beyond the fifth on demand, the call-site list beside the tree, scope filters (the
language server answers for the project it indexed), the type and method hierarchies, pinning
several hierarchies as tabs (each new hierarchy replaces the shown one, as Find Usages does).

## Known deviations from the reference
The reference builds the hierarchy from its own index; Ferris IDEA asks the language server, so results
depend on the server and on what it has indexed (with clangd: the open file and indexed files of the
project).

v5 (OBS-0027): the reference's window froze for over a minute on Run while a background engine
started, and Go to Implementation did not answer meanwhile; Ferris IDEA's hierarchies, Super Method and
Implementations run in the background and never block the window (IMP-0380). The method hierarchy
marks the type it was asked for with `+`/`-`/`!` like the others (the reference adds `*`).

v6 (OBS-0035): Kotlin hierarchies know only the project's own `.kt` files. Library types are leaves
and their subtypes in other libraries are not listed; Java classes of a mixed project are not part of
a Kotlin hierarchy; a type brought in under another name (`import a.B as C`, a type alias) is taken
for a library type; overloads with the same number of parameters count as one member. The reference
resolves all of these from its index. In return the answer does not wait for the build import: the
reference showed its Kotlin analysis about nine minutes after opening the project (IMP-0399).

v7 (OBS-0071): the reference's call tree over MCP names places outside the project relative to it,
with `..` into the JDK's source archive; Ferris IDEA keeps such places absolute and makes only places in
the workspace relative (IMP-0445).

v8 (OBS-0071): the reference lists the constructor of a record among the callees; Ferris IDEA adds it from
the syntax tree (behaviour 19) and names it as the type, as jdtls names a class's implicit constructor.
A record's implicit accessors (`r.name()`) are still left out by jdtls and not added.
