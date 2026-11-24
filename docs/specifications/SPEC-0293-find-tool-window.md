# SPEC-0293: Find tool window for Find Usages

- **Version:** 12 · **Date:** 2026-11-19 (v12: Go usages are grouped by their syntax — receivers, struct initializations, types, calls — and the mentions of a Go function, method or type in comments are a group of their own, counted apart; the symbol's own declaration is never listed, also when the server names no declaration and in a text answer; behaviours 14b, 29 and 30, AK-18 and AK-19; side-by-side comparison of 2026-11-19, F-038 gaps GO1 and RS1); 2026-11-17 (v11: without a language server for the file the window and MCP list the text occurrences with the reason, and in JavaScript and TypeScript a text occurrence inside a string or a comment is no usage, behaviours 10a and 11, AK-9 and AK-17; found on a CI runner without Node language servers, where `find_usages` failed instead of answering from the text as AK-9 asks); 2026-11-16 (v10: Find Usages with a choice of scope, behaviour 28, AK-16; eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v9: the filters by read access, write access and import statements, the nested folders, the module groups, Exclude and Include, and the export to a text file, under the ids of the bundled keymaps, behaviours 22–26, AK-14 and AK-15; sixth package of docs/research/KEYMAP_COVERAGE.md; v8: a place without a name is not searched and a search never stays in its searching state, behaviours 20 and 21, AK-13; window check of 2026-11-14: Find Usages in File at the first column of an annotated line opened the window searching for a symbol without a name); 2026-11-13 (v7: `FindUsagesInFile` shows only the usages in the file of the caret, behaviour 19, AK-12; v6: the grouping toggles under the ids of the bundled keymaps, behaviour 18, AK-11; docs/research/KEYMAP_COVERAGE.md; v5: a class or id selector of a stylesheet also lists its uses in HTML files, from Ferris IDEA's own index and marked so, behaviour 17, AK-10; v4: in JavaScript and TypeScript the kinds come from the syntax tree and the members that implement or override the searched one are no usages, OBS-0028 T5, behaviours 14a and 16, AK-9); 2026-11-05 (v2: only the name is marked, OBS-0016; v3: grouped by usage kind, module, package, class and method, OBS-0016 defect F034-3)
- **Sources:** OBS-0012 (side-by-side 2026-11-05 with `Inventory.java` of `java-basic`, record `Item`:
  ⌥F7 in the reference opens a tool window at the bottom, not a popup; its tab is named after the
  search, "<symbol> in Project Files"; a tree starts with the searched symbol, then a group of usages in
  project files with a count, then the files with their usages; a preview on the right shows the
  selected usage in its file with the usage marked; a vertical toolbar on the left offers rerun,
  previous/next, settings and grouping toggles. On the JDK type `String` in a parameter the reference
  answers at once, Ferris IDEA said "Finding usages of String…" and then showed nothing at all for more
  than 25 s), OSS (action ids `FindUsages` ⌥F7, `ShowUsages` ⌥⌘F7, `PreviousOccurence` ⌥⌘↑,
  `NextOccurence` ⌥⌘↓, `HideActiveWindow` ⇧Esc of the ported keymaps), STD (LSP 3.17
  `textDocument/references` with `includeDeclaration`, `textDocument/definition`, `$/cancelRequest`),
  SPEC-0073 (usages popup, now Show Usages only), SPEC-0082 (Hierarchy tool window, same place),
  SPEC-0029 (whole-word text search), SPEC-0071 (navigation history), SPEC-0090 (language server pool)
- **Feature matrix rows:** F-034

## Purpose
Find every use of a symbol in the project and work through them one by one, with the code around
each use in view, without the list closing when you jump.

## Behaviour
1. **Start:** `FindUsages` (⌥F7) on the symbol at the caret opens the Find tool window at the bottom of
   the editor area at once, in a searching state: its header names the search `<symbol> in Project
   Files`, the body says it is searching and how many seconds have passed, and a Stop button is shown.
   The status bar says `Finding usages of <symbol>…`. A new ⌥F7 replaces the window's content. [OBS-0012,
   OSS]
2. **Search:** the file's language server is asked, on the editor's current text, for the declaration
   of the symbol (definition) and for its references without the declaration. The whole search has one
   deadline of 20 s, server start included; the server's own request timeout never lengthens it. When
   the deadline passes or Stop is pressed, the open request is withdrawn with `$/cancelRequest`, so the
   server does not keep working on it. [STD, GEN]
3. **Project files only:** usages in files outside the project folder (library sources, the JDK,
   `jdt://` class files) are not listed. [OBS-0012, STD]
   *Why the JDK type was silent (measured 2026-11-05 with jdtls on `java-basic`):* references of
   `String` took about 30 s (15 s without class-file content support, which navigation into the JDK
   needs), those of `Item` 3 s, server start included; the pooled server kept the request timeout of
   whoever started it (60 s), so the 30 s asked for by ⌥F7 did not apply, and a timeout was reported as
   "no usages". Behaviours 2, 10 and 11 close these gaps. [GEN]
4. **Tree:** the first row is the target: the symbol's name with the declaration's file and line, or,
   for a declaration outside the project (a library or JDK type), the name the server gives it. Below it
   a group row `Usages in Project Files` with the count (`1 usage`, `N usages`). Below that one row per
   file — file icon, workspace-relative path and its count — and below each file its usages as
   `<line>  <trimmed line text>` with the symbol in the text marked. Files are sorted by path, usages
   by line and column, duplicates dropped. Everything is expanded at first and the first usage is
   selected. [OBS-0012, GEN]
5. **Group by file:** a toolbar toggle (on by default) switches between the files level and a flat
   list of usages under the group row, each with `path:line` after its text. [OBS-0012]
6. **Preview:** the right part of the window shows the selected usage's file around its line — up to 6
   lines before and after, with line numbers — from the open editor's text when the file is open, else
   from disk; the usage's line has a highlighted background and the symbol in it is marked. On a file
   row it shows that file's first usage; on the target or group row the first usage of all. A toolbar
   toggle hides and shows the preview (shown by default). [OBS-0012]
7. **Toolbar:** vertical, at the left edge: Rerun (searches again for the same symbol at the same
   place), Stop (only while searching), Previous and Next usage, Expand All, Collapse All, Group by
   File, Preview, and Close. [OBS-0012, GEN]
8. **Keys:** Up/Down select, Right expands (or moves into), Left collapses (or moves to the parent),
   Enter or a double-click on a usage opens its file at the usage and records the place left for Back
   (SPEC-0071) while the window stays open; ⌥⌘↓/⌥⌘↑ (`NextOccurence`/`PreviousOccurence`) go to the
   next/previous usage and open it, also from the editor while the window is open, wrapping at the
   ends. Escape gives the focus back to the editor; ⇧Esc and Close hide the window. [OSS, GEN]
9. **No usages:** when the server answers with no usage in project files, the window says
   `No usages of <symbol> in project files` and the status bar says the same. [GEN]
10. **Server too slow:** when the deadline passes without an answer, the window lists the whole-word,
    case-sensitive text occurrences of the symbol in project files with the same file extension
    (SPEC-0029 search, at most 1000), grouped like usages, under a group row `Text occurrences in
    Project Files` with a note that the language server did not answer within 20 s; without any
    occurrence it says that no usages were found and why. The status bar says the same. [GEN]
    10a. **No language server (v11):** when no usable language server is available for the file
    (none of its servers is installed, or Ferris IDEA knows none for its language), the window lists
    the text occurrences as in behaviour 10, under the same group row and marked the same way (MCP
    `source` `text`), with a note that gives the reason in the words of the server lookup — which
    server is missing and how to install it, e.g. `No usable language server for typescript:
    typescript-language-server not found (install: …); showing text occurrences.` — and the status
    bar says the same. Without any occurrence it says that no usages were found and this reason.
    Before v11 the search failed with the reason only, which contradicted AK-9. In `.ts`, `.tsx`,
    `.mts`, `.cts`, `.js`, `.jsx`, `.mjs` and `.cjs` files a text occurrence (behaviours 10 and 10a)
    inside a string, a template string's text or a comment is left out, like the member
    declarations of behaviour 16: a language server never lists it as a usage. [GEN]
11. **Stopped / failed:** after Stop the window says the search was stopped; when the search fails
    for another reason (a server error, a server that cannot be started) it gives the reason (v11:
    no language server for the file is behaviour 10a). Nothing stays in a searching state after the
    deadline. [GEN]
12. **MCP:** `find_usages` (path, line, character; optional `timeout_s`, 1–60, default 20) runs the same
    search on the saved file and returns the target, the usages grouped by file (path, line,
    character, line text), whether they came from the language server or the text fallback, and the
    note. [ADR-0008]
13. **The name is marked (v2):** in a usage's line and in the preview only the symbol's name is
    marked, also when the server's range covers more (jdtls reports a method call from the name to
    its closing parenthesis, so `apply(item.totalCents())` was marked whole); the reference marks the
    name only (OBS-0016 Nr. 5). [OBS-0016]

14. **Grouping (v3):** below the group row the usages are grouped, outer to inner, by
    - **usage kind**, from the usage's line (own rules, any language): `Imports` when the line is an
      import (`import`, `#include`, `use`, `from … import`, `require`); `Calls` when the next
      character after the name, spaces skipped, is `(`; `Writes` when it is an assignment (`=` not
      followed by `=`, `+=`, `-=` and the other compound forms, `++`, `--`); else `Other usages`;
    - **module**: the nearest folder holding a build file, as in SPEC-0081 v2 behaviour 4, shown
      with its folder name;
    - **package**: the file's package symbol (Java), else its namespaces, else the folder below the
      module (no row for a file directly in the module folder);
    - **file**, with its icon, path below the package and count — left out when the class level
      is on and the usage lies inside a type, so a Java usage reads module › package › class ›
      method as in the reference;
    - **class**: the type around the usage with its enclosing types (`Discounts.Half`), and
      **method**: the innermost function, method or constructor around it with the server's name
      (`priceOf(Item, Discount)`); a usage outside either has no such row.
    Groups are sorted by their label, usages in a group by path, line and column; every group row
    shows its count. Module, package, class and method come from the symbols of each usage's file,
    asked of the server within the search's deadline (at most 100 files, the rest without class and
    method rows). The toolbar has a toggle per level besides Group by File: Group by Usage Kind,
    Group by Module, Group by Package, Group by Class and Method, all on by default; ⌥⌘↑/⌥⌘↓ and the
    preview follow the shown order. The target row shows the symbol's kind before its name (`method
    apply`), from the declaration's symbol. The reference groups by target kind, usage kind,
    module, package, class and method (OBS-0016 Nr. 8); Ferris IDEA grouped by file only. [OBS-0016,
    STD, GEN]
    14a. **Kinds from the syntax tree (v4):** in `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`,
    `.mjs` and `.cjs` files the kind is read from the syntax tree instead of the line: `Calls` for
    the function of a call, of `new` or a decorator (also `this.area()`, `a?.area()`), `Writes` for
    the target of an assignment, a compound assignment, `++`/`--` or a `for … in/of` variable,
    `Imports` inside an import, `Type usages` in a type annotation, type arguments, `extends` and
    `implements`, `Declarations` for the name of a function, class, interface, type, enum or
    variable declared there (an overload signature), `Reads` for any other use. Before v4 a line
    rule saw `(` after the name and called `area(): number {` a call. Other languages keep the
    line rule. The group order is imports, calls, writes, reads, type usages, declarations,
    others. [OBS-0028 item 7, STD]
    14b. **Go kinds from the syntax tree (v12):** in `.go` files the kind is read from the syntax
    tree: `Receivers` for the type of a method's receiver (`func (r Rect) Area()`, also `*Rect`),
    `Struct initializations` for the type of a composite literal (`Rect{W: 2, H: 3}`, `&Rect{}`,
    `pkg.Rect{}`), `Calls` for the function of a call (a conversion `Rect(x)` included), `Writes`
    for the left side of an assignment and for `++`/`--`, `Imports` inside an import declaration,
    `Declarations` for the name of a type, function, method, constant, variable or field declared
    there, `Type usages` for any other place where a type is named (a parameter, a result, a
    variable's type, a slice's element), `Usages in comments` for a place inside a comment
    (behaviour 30); a place inside a string is text, no usage (as in behaviour 10a); everywhere
    else the line rule stays. The group order is imports, calls, writes, reads, type usages,
    receivers, struct initializations, declarations, others, usages in comments. MCP kinds:
    `receiver`, `struct_initialization` and `comment`. Before v12 gopls's two usages of a struct
    type were both `Other usages`; the reference groups them as receiver and struct
    initialization. [F-038 GO1, GEN]
15. **MCP (v3):** each usage of `find_usages` carries `kind` (`import`, `call`, `write`, v4 also
    `read`, `type` and `declaration`, else `other`), `module`, `package`, `class` and `method` when
    known; the target carries `kind`. [ADR-0008, IMP-0301]
16. **Implementations are no usages (v4):** in JavaScript and TypeScript files a place that is the
    name of a method, field, accessor or property signature of a class, an interface or an object
    literal is the declaration of another member of the same name — one that implements or
    overrides the searched member — and is left out of Find Usages (window and MCP). Find Usages on
    `area` of `interface Shape`, implemented by `abstract class Base` and overridden in `Circle` and
    `Square`, lists the one call in `Base.describe`, as the reference does; typescript-language-
    server and TypeScript 7's `tsc --lsp` both answer `textDocument/references` with the three
    declarations besides the call (checked with typescript-language-server 5.3.0, TypeScript 5.6.3
    and 7.0.2). The implementations stay reachable through Go to Implementation and the type
    hierarchy (SPEC-0320); Rename still renames all five places (SPEC-0074). The same applies when
    the server is too slow and the text occurrences are listed (behaviour 10). Python: pyright
    1.1.390 answers with the call only, so nothing changes for Python; Java and Kotlin were not
    checked against their servers (not installed on the test machine) and keep behaviour 14.
    Usage counts (SPEC-0292) still count every place the server names. [OBS-0028 item 7, GEN]
17. **Selectors and HTML (v5):** on a class or id selector of a CSS, SCSS or Less file the usages
    the stylesheet's server names are kept, and the `class` words, for an id the `id` values,
    that equal the name in the project's own HTML files are added (SPEC-0042 v17 behaviours 30
    and 32). These come from Ferris IDEA's own index, not from a language server: the note of the
    search says how many they are, and in `find_usages` each carries `source: "own_index"`.
    They are sorted with the others by path, line and character and grouped like them
    (behaviour 14). [GEN, SPEC-0042]
18. **Grouping actions (v6):** the toolbar's grouping toggles of behaviour 14 exist under the ids the bundled keymaps bind: `UsageGrouping.UsageType` (⌃T) flips Group by Usage Kind, `UsageGrouping.Module` (⌃M) Group by Module, `UsageGrouping.Directory` (⌃P) Group by Package (the level is the package or, without one, the folder) and `UsageGrouping.FileStructure` (⌃S) Group by Class and Method. Their keys act only while the Find tool window has the keyboard focus; everywhere else the keys belong to other actions. Chosen in Find Action they act on the window's result wherever the focus is; without a result the status line says that there is none to group. Find Action lists them with the group "Find". They are view state of the window and get no MCP tool: `find_usages` gives every usage with its kind, module, package, class and method (behaviour 15), so a caller groups as it likes. Not carried out, because the window has no such operation: `UsageGrouping.DirectoryStructure` (a nested folder tree), `UsageGrouping.FlattenModules`, the filters `UsageFiltering.ReadAccess`, `UsageFiltering.WriteAccess` and `UsageFiltering.Imports`, and `UsageView.Include` (rows cannot be excluded). [OSS: ids and shortcuts of the bundled keymaps; GEN]
19. **Find Usages in File (v7)** (`FindUsagesInFile`, Ctrl+F7, ⌘F7 on macOS; Find Action lists it as
    "Find Usages in File" under Edit): the search of behaviour 2 for the symbol at the caret, shown
    in this tool window like any other search, with only the usages that lie in the file of the
    caret. The tab is named `<symbol> in <file name>` and the group row `Usages in <file name>`
    (`Text occurrences in <file name>` for behaviour 10), with the count of the usages in the file;
    grouping, preview, stepping (⌥⌘↓/⌥⌘↑), Stop and Rerun work as for a search in the project, and
    Rerun searches in the file again. Without a usage in the file the window and the status bar
    say `No usages of <symbol> in <file name>`; a note of the search (behaviours 10 and 17) is
    shown as it is. The declaration is a usage only when the server lists it as one, as in the
    project search. The same question is put to the server as for the project; the usages in
    other files are left out of the answer. `find_usages` takes `in_file: true` for the same
    restriction. [OSS (action id and keys of the ported keymaps), GEN, ADR-0008]
20. **No name at the caret (v8):** `FindUsages`, `FindUsagesInFile` and `ShowUsages` first look
    for the name at the caret with the rule the tab's title is made with: the letters, digits and
    underscores around the caret, also when the caret stands right after them. Where there is
    none — before the `@` of an annotation, in white space, on an empty line, between signs or
    the signs of a comment — there is nothing to search for: the status bar says `No usages
    found: the caret is not on a name`, the words `HighlightUsagesInFile` uses for the same case
    (SPEC-0295 behaviour 8), the Find tool window is not opened and, when it is open, keeps the
    search it shows; no popup opens and no language server is asked. Before v8 the window opened
    and said that it was searching for the usages of a symbol with an empty name. Whether a word
    is a symbol is not decided here: on a word in a comment or a string the server is asked as
    before, and behaviours 9 to 11 say how that ends. `find_usages` refuses a position without a
    name as it did before v8. [GEN, window check 2026-11-14]
21. **A search always ends (v8):** the window leaves its searching state with every end of a
    search: a tree, or one sentence. A search that ends with a result without a single usage
    shows `No usages of <symbol> in project files` (in a file: `… in <file name>`) instead of
    an empty tree. A search that has not reported back 40 s after its start — the deadline of
    behaviour 2 and as much again for the text search of behaviour 10 — is ended by the window
    itself: it withdraws the search as Stop does and says that the search did not end within
    40 s and that Rerun searches again. [GEN]
22. **Filters (v9):** three toggles of the toolbar decide which usages the tree holds; all three
    are on at first, and like the grouping they stay as set for the next search. They stand,
    with the buttons of behaviours 23, 24 and 26, in a second column beside the toolbar, so the
    first column is no longer in a low tool window than it was.
    `UsageFiltering.ReadAccess` (Show Read Access, ⌃R, Ctrl+R in the default keymap) shows the
    usages known to read the symbol, `UsageFiltering.WriteAccess` (Show Write Access, ⌃W,
    Ctrl+W) those that write it, `UsageFiltering.Imports` (Show Import Statements, ⌃I, Ctrl+I)
    those in an import statement. As for the grouping toggles of behaviour 18 the keys act
    while the window has the focus and go before the other actions on the same chords (Run,
    Debug, Implement Methods). A usage is hidden only when its kind is known and its toggle is off: a
    call, a type usage, a declaration and a usage of unknown kind are always shown. A hidden
    usage is in no row, in no count, and Next and Previous pass it; the group row says how many
    usages remain, and with every usage hidden it stays, with `0 usages`, so the tree never
    looks like a search without a result. The kinds are those of behaviour 14 (the line's text,
    or the syntax tree where v4 has one). [OSS: ids, titles and the rule "hidden only when the
    usage is known to be of that kind and the toggle is off" from `ReadAccessFilteringRule.kt`,
    `WriteAccessFilteringRule.kt`, `rules/ImportFilteringRule.java` and `UsageViewBundle.properties`
    of the open platform, logic template; GEN]
23. **Directory Structure (v9):** `UsageGrouping.DirectoryStructure` (⌃D, Ctrl+Alt+D in the
    default keymap) replaces the one
    package row of behaviour 14 by one row per folder of the usage's file, nested from the outside
    in, each with its count, a folder before the files beside it; the folder of the module itself
    is not repeated below the module's
    row. Off (the start), the package row is as before. The toggle works whether Group by Package is
    on or off. [OSS: id, title and "one group per directory from the base directory down" from
    `DirectoryStructureGroupingRule.java`, logic template; the open platform can also join the
    middle folders into one row, Ferris IDEA never does]
24. **Flatten Modules (v9):** `UsageGrouping.FlattenModules` (⌃O, Ctrl+Alt+O in the default
    keymap) is on at first: every module is one row. Off, a module whose name has dots sits
    below one group row per leading part of its
    name (`shop.api.rest` below `shop` and `shop.api`), so modules of one family stand together.
    The toggle acts only while Group by Module is on; without it the action changes nothing and
    its key is Export to Text File's (behaviour 26). [OSS: id, title,
    "one group per prefix of the module's group path" and "enabled only with Group by Module" from
    `ModuleGroupingRule.java` and `FlattenModulesAction.java`, logic template. Ferris IDEA knows a
    module by its folder name only; the dotted name is the only group path it has]
25. **Exclude and Include (v9):** `UsageView.Exclude` (Delete and Backspace in the window; the
    keymap files bind no key, the two are Ferris IDEA's and listed as deviations of the keymap)
    excludes the selected usage, or every usage below the selected group (all of them on the
    row `Usages in …`, none on the row of the searched symbol); `UsageView.Include`
    (⇧⌫, Insert in the default keymap) takes that back. An excluded usage stays in the
    tree, struck through and dimmed; it is in no count, Next and Previous pass it, and the
    export leaves it out. A group whose usages are all excluded is struck through too. After
    excluding one usage the next usage shown below it is selected, so the key excludes one usage
    after the other and never a group by accident. The action that would change nothing says
    so in the status bar. A rerun of the same
    search keeps the excluded usages that are found again; another search starts with none.
    Nothing is removed from the search, and no refactoring reads the exclusions (Ferris IDEA's
    refactorings have previews of their own). [OSS: ids, titles and "applies to every node
    below the selection that can be excluded and is not yet" from `TreeNodeExclusionAction.java`
    and `ExclusionHandler.java`, logic template; GEN for the selection moving on and the rerun]
26. **Export to Text File (v9):** `ExportToTextFile` (a button of the toolbar; Alt+O in the
    default keymap; in the macOS keymap ⌃O, the chord of Flatten Modules, which goes first, so
    the key exports only while Group by Module is off) asks for a file in
    the system's save dialog, starting in the project folder with `usages-of-<symbol>.txt`, and
    writes the tree as plain text: the symbol, the group row with its count, then every row of
    the fully expanded tree, two blanks deeper per level, each group with its count. Excluded
    usages and what a filter hides are not written. The status bar names the file and the number
    of usages; the file is not opened. Without a result the action says so, as the actions of
    behaviours 22–25 do. The reference
    exports other views too (hierarchies, the TODO list); Ferris IDEA exports this window only.
    [OSS: id and title; GEN for the format]
27. **Agents (v9):** UI automation runs the actions of behaviours 22–26 by their ids and knows
    what each changes: the filters, groupings, Exclude and Include change the view only, Export
    to Text File writes a file. `find_usages` is unchanged: it returns every usage with its
    kind, so an agent filters itself. [ADR-0008]
28. **Find Usages with its settings (v10):** `ShowSettingsAndFindUsages` (⌥⇧⌘F7 in the bundled
    macOS keymap) on a name asks for the scope in a list titled "Find Usages of <name>":
    "Project Files" searches as `FindUsages` does (behaviour 1), "This File" as
    `FindUsagesInFile` (v7 behaviour 19). Without a name at the caret nothing opens and the
    status bar says so (v8 behaviour 20). [GEN]
29. **The declaration is never a usage (v12):** the symbol's declaration is the target row, not a
    usage, in every language and for every answer. The server is asked without the declaration
    (`includeDeclaration: false`, behaviour 2); in addition a place on the declaration's line
    whose range meets the declaration's range is left out, so a server that names its
    declaration from the line's first column (`struct Rect {`) or names it anyway is covered.
    When the server names no declaration (no answer to `textDocument/definition` within its wait,
    an empty answer, a server still loading) the declaration is taken from the syntax tree: the
    declared name under the caret. In a text answer (behaviours 10 and 10a) the declaration is the
    declared name under the caret, else the only declaration of that name among the occurrences;
    with two or more (a trait method and its implementation) none is taken and all stay listed.
    Declared names come from the grammar's tags query and from the `name` of every node that
    declares or defines something. Before v12 a text answer on `struct Rect` in Rust listed the
    declaration first (3 places where the reference shows the `impl` and the literal). [F-038
    RS1, STD, GEN]
30. **Usages in comments (v12, Go):** when the target is a function, method or type declared at
    the top of a Go file of the project, the whole-word, case-sensitive mentions of its name in
    the comments of the project's Go files (`// Rect is a rectangle.`) are added under their own
    group `Usages in comments` (kind `comment`), one per place; a language server never names
    them. They are listed and shown like usages (preview, Next/Previous), the group row counts
    them, and the group row above all usages counts them with the code usages. MCP
    `find_usages` lists them in `files` with kind `comment`; its `count` counts the code usages
    only and `comment_count` the mentions in comments, so an agent that counts usages is not
    misled. Parameters and local names get no comment mentions (a short name would match words
    of any comment). Usage counts at the lines (SPEC-0292) do not count them. Other languages:
    not looked at yet. [F-038 GO1, GEN]

## Acceptance criteria
- **AK-1** Tree model: grouping by file, counts, labels, sorting, de-duplication, flat mode, expand and
  collapse, next/previous with wrap-around, the target row for a declaration inside and outside the
  project (unit tests).
- **AK-2** Preview model: the lines around a usage, the usage line and the marked range, clamped at the
  start and end of the file (unit test).
- **AK-3** Bounded search: a request without an answer ends at the deadline with a timeout and sends
  `$/cancelRequest` for it; a stop flag ends it the same way; places outside the project are dropped;
  the text fallback finds whole-word occurrences only in files of the same extension (unit tests with
  a scripted server).
- **AK-4** MCP `find_usages` is listed with its schema and refuses a `timeout_s` outside 1–60
  (protocol test).
- **AK-5** In a real window (`ferris-idea ui --selftest`, with clangd): ⌥F7 on `helper` opens the Find
  tool window titled `helper in Project Files` with its usages grouped under the file; ⌥⌘↓ opens the
  next usage; ⇧Esc hides the window; ⌥⌘F7 still opens the usages popup.
- **AK-6** Side by side with the reference (OBS-0012): ⌥F7 on `Item` and on `String` in
  `Inventory.java`; the JDK type answers within 20 s.
- **AK-7 (v2)** A usage range wider than the name marks the name only, in the hit and in the preview
  (unit test `find_usages_model::tests::only_the_name_is_marked`).
- **AK-8 (v3)** Usage kinds from the line (unit test `context::tests::usage_kinds_from_the_line`);
  the tree groups by kind, module, package, class and method, drops the file level inside a type,
  counts every group, follows the toggles and steps in shown order (unit tests
  `find_usages_model::tests::groups_by_kind_module_package_class_and_method`,
  `find_usages_model::tests::levels_can_be_switched_off`); with jdtls the call of `apply` in
  `Discounts.priceOf` has kind `call`, package `lab.inventory`, class `Discounts` and method
  `priceOf(Item, Discount)` (live test `tests/java_navigation.rs`); in a real window ⌥F7 on
  `helper` shows target, group, `Calls`, the module, `bad.c`, `main` and the usage (window
  self-test).

- **AK-9 (v4, behaviours 14a and 16)** Kinds and member declarations from the syntax tree in `.ts`,
  `.tsx`, `.js` and `.jsx` (unit test `script_usages::tests::kinds_from_the_syntax_tree`); Find
  Usages on `area` of the interface lists only the call in `describe`, kind `call`, with
  typescript-language-server and with TypeScript 7's `tsc --lsp` (live test
  `crates/languages/tests/it/ts_usages.rs::usages_of_an_interface_method_are_its_calls`; without the
  filter both list the four places of OBS-0028); MCP `find_usages` the same, also from the text
  without a server (`protocol.rs::find_usages_of_an_interface_method_lists_its_calls`; v11: with
  `source` `text` when no TypeScript server is installed, behaviour 10a).
- **AK-10 (v5, behaviour 17)** SPEC-0042 AK-32: with vscode-css-language-server, Find Usages on
  `.card` of `css/site.css` lists `class="card"` of `index.html` marked as from the own index,
  with the note (`crates/languages/tests/it/html_css.rs::css_selector_usages_include_html`,
  skipped without the server); the merge itself without a server
  (`find_usages::tests::own_index_usages_are_added_once`).
- **AK-11 (v6, behaviour 18)** Each id names its toggle and no other id does (unit test `keymap_actions::tests::usage_grouping_actions_name_their_toggle`); the keys run only with the focus in the Find tool window (`keymap_actions::tests::scoped_actions_run_first_and_only_in_their_focus`); the titles and the group (`everywhere::tests::navigation_actions_from_the_keymap_have_titles`). In the window: open.
- **AK-12 (v7, behaviour 19)** Restricting a search to a file keeps the usages of that file with
  their details and drops the others
  (`find_usages::tests::a_search_is_restricted_to_one_file`); the tree's group row and the
  window's title name the file (`find_usages_model::tests::a_file_search_names_the_file`);
  `find_usages` lists `in_file` in its schema (`protocol.rs`: `find_usages_takes_in_file`). By
  hand in a window, not done yet: ⌘F7 on a symbol used in two files.
- **AK-13 (v8, behaviours 20 and 21)** A place without a name gives the status text and no
  search; a name, also in a comment or right before the caret, gives the search
  (`usages_model::tests::a_place_without_a_name_is_not_searched`); Highlight Usages in File says
  the same words (`occurrence_model::tests`, the `End::NoName` case). In a window of the test
  harness each of the four actions, at the `@` of an annotation, in the indentation, between the
  signs of a comment and on an empty line, sets the status text and opens neither the Find tool
  window nor the usages popup, and on a name Find Usages in File still opens the window
  (`crates/workbench/tests/it/usages_without_symbol.rs`). A result without a usage and a search
  running for 40 s end with their sentence
  (`find_usages_model::tests::a_search_without_a_result_ends_with_words`). Not checked in a
  window with a language server; the 40 s end is checked as a rule only, not by waiting.
- **AK-14 (v9, behaviours 22–25)** On a tree with read, write, import and call usages in two
  modules: each filter hides its kind only and the counts follow; with everything hidden the
  group row stays with `0 usages`; Directory Structure gives one row per folder and does not
  repeat the module's folder; Flatten Modules off nests a dotted module name and does nothing
  without Group by Module; Exclude on a usage and on a group, Include, the counts, the rows
  struck through, Next and Previous passing excluded usages, and a rerun keeping them
  (`find_usages_model::tests::filters_hide_only_their_kind`,
  `find_usages_model::tests::nested_folders_and_module_groups`,
  `find_usages_model::tests::excluded_usages_are_passed_and_not_counted`). The ids name their
  change, are limited to the window, have their keys in the bundled keymaps and go before
  the other actions on their chords while the window has the focus
  (`keymap_actions::tests::usage_view_actions_name_their_change`). In a window of the test
  harness each action changes the tree shown, the key of Exclude excludes the selected row
  while the window has the focus, and the actions without a result say so
  (`crates/workbench/tests/it/sixth_package_keymap.rs::find_window_filters_groups_and_excludes`).
  Not compared next to the reference.
- **AK-15 (v9, behaviour 26)** The exported text of a tree with an excluded and a filtered usage
  (`find_usages_model::tests::the_export_leaves_out_what_is_excluded_or_filtered`); the offered
  file name and the status text (`keymap_actions::tests::usage_view_actions_name_their_change`);
  in a window of the test harness the file is written and named in the status bar, and without
  a result nothing is written (`sixth_package_keymap.rs::usages_are_exported_to_a_text_file`).
  The save dialog itself is not exercised by a test.
- **AK-16 (v10, behaviour 28)** Window test `eleventh_package_keymap.rs`:
  `find_usages_asks_for_the_scope` (title and the two scopes, macOS).
- **AK-17 (v11, behaviour 10a)** Without a usable language server the search of `area` of the
  interface of AK-9 answers with the call in `describe` only, kind `call`, source `text` and a
  note that names the missing server, its install hint and that text occurrences are shown
  (unit test `find_usages::tests::without_a_server_text_occurrences_with_the_reason`); a text
  occurrence in a string or comment of a TypeScript file is left out
  (`script_usages::tests::strings_and_comments_are_no_usages`); the MCP test of AK-9 passes with
  a `PATH` without typescript-language-server, vtsls and tsc and with them. The window's words
  for a search without occurrences name the reason
  (`workbench_find_usages::tests::nothing_found_names_the_reason`).

- **AK-18 (v12, behaviours 14b and 30)** Go kinds from the syntax tree
  (`go_usages::tests::go_kinds_from_the_syntax_tree`); with gopls, Find Usages on `Rect` asked on
  the declaration and on the struct initialization lists the doc comment's mention as `comment`,
  the receiver as `receiver` and the literal as `struct_initialization`, two code usages, the
  declaration not listed
  (`crates/languages/tests/it/side_by_side_f038.rs::gopls_usages_by_kind_with_the_comment_mention`,
  skipped without gopls); the text answer the same without a server
  (`find_usages::tests::a_text_answer_leaves_out_the_declaration`).
- **AK-19 (v12, behaviour 29)** A place meeting the declaration on its line is the declaration
  (`find_usages::tests::the_declaration_is_matched_by_its_range`); a text answer in Rust and Go
  leaves out the declaration asked on it and on a usage, and keeps two declarations of one name
  (`find_usages::tests::a_text_answer_leaves_out_the_declaration`); declared names in Rust, Go and
  PHP (`declared_names::tests::declarations_in_three_languages`); with rust-analyzer, Find Usages
  on `struct Rect` asked on the declaration and on `impl Rect` lists the `impl` and the literal
  only (`side_by_side_f038.rs::rust_analyzer_usages_without_the_declaration`, skipped without
  rust-analyzer), with gopls the same in AK-18. Not checked in a window.

## Out of scope
Several result tabs side by side (each new search replaces the content), the settings dialog of Find
Usages (⌃⌥⇧F7: scope, options, `ShowSettingsAndFindUsages`), removing rows from the result
(`UsageView.Remove`), filters for generated code and comments, joining the middle folders of
Directory Structure, the export of other views, merging same-line usages, a stripe button for the window, usages in non-code files,
read/write analysis by the language server (the usage kind follows the line's text), a tab for the
call hierarchy below the preview.

## Known deviations from the reference
- The window keeps one search; the reference keeps a tab per search.
- Results come from the language server, not from an own index; when the server is slower than 20 s
  Ferris IDEA shows text occurrences marked as such instead of waiting longer.
- The toolbar has no settings button yet.

- v5: usages of a selector in HTML files come from Ferris IDEA's own index of `class` and `id`
  attributes; classes that scripts or templates build are not found, and a search started on the
  class in the HTML file still asks the HTML server only. Not compared next to the reference.
- v7: Find Usages in File is not compared with the reference. Open there: how it names the search,
  whether it marks the usages in the editor as well (Ferris IDEA: `HighlightUsagesInFile`, SPEC-0295)
  and whether it searches without the project (Ferris IDEA asks the server the project question and
  leaves out the other files, so it is no faster than Find Usages).
- v9: the filters, Directory Structure, Flatten Modules, Exclude, Include and the export follow
  the open platform's actions as far as their sources state a rule; none was looked at in the
  reference. Known differences: read and write access come from the usage kinds of behaviour 14,
  not from a language analysis; module groups come from dotted folder names; excluded usages
  are not skipped by a refactoring, because no refactoring of Ferris IDEA runs from this window.
- **Go usage kinds (v12):** the group labels are plural like the other kinds (`Receivers`,
  `Struct initializations`, `Usages in comments`); the reference names them in the singular.
  Comment mentions are searched for Go only and only for functions, methods and types; the
  reference's rule for other languages and other symbols was not looked at. MCP counts the
  mentions apart from the code usages (behaviour 30); the reference's tree counts them together,
  as Ferris IDEA's window does.
- **Find Usages settings (v10):** the reference's dialog also offers the kinds of usages and
  further scopes; Ferris IDEA asks only for the two scopes it can search (the filters of v9 work
  on the result).
