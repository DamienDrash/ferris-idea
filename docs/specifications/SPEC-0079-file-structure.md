# SPEC-0079: File Structure popup

- **Version:** 7 · **Date:** 2026-11-13 (v7: HTML files nested as the document, elements named with id and classes, in document order, OBS-0039 W3; v2: below the caret, full-width selection; hand test; v3: no package row, OBS-0016; v4: order by kind and name, nested types closed, filters for inherited members, anonymous classes and lambdas, filter field, matches highlighted, OBS-0016 defects F034-4 and F034-5; v5: Python functions with their parameter list, no parameters and local variables, OBS-0034 L11; v6: the same for JavaScript and TypeScript, with the types the source states, OBS-0028 T1)
- **Sources:** GEN (a popup with the symbols of the current file as a tree, filtered by typing; F-034),
  OSS (action id `FileStructurePopup` with ⌘F12 / Ctrl+F12 in the ported keymaps; symbol icons from
  `platform/icons/src/expui/nodes/`), STD (LSP `textDocument/documentSymbol`), SPEC-0024 (symbols
  through the language server), SPEC-0072 (requests on unsaved text), OSS (the kind order of the
  popup in `java/java-structure-view/src/com/intellij/ide/structureView/impl/java/KindSorter.java`
  and the shortcuts ⌘I / Ctrl+I and ⌘L / Ctrl+L of `JavaAnonymousClassesNodeProvider.java` and
  `JavaLambdaNodeProvider.java`, as a logic template), DOC (pressing the shortcut of the popup again
  shows inherited members), STD (LSP 3.17 `textDocument/prepareTypeHierarchy`,
  `typeHierarchy/supertypes`), OBS-0016 (side by side 2026-11-05, Java); v5: OBS-0034 item 43 (for a
  Python file the reference lists functions and methods with their parameter list, classes with
  their methods and fields, and neither parameters nor local variables), STD (The Python Language
  Reference, "Function definitions": the forms of a parameter list); v6: OBS-0028 "Editorfunktionen
  neben der Referenz, Teil 1" item 1 (T1: for a TypeScript file the reference lists functions and
  methods with their parameters and types, fields with their type, an arrow function bound to a
  constant as a function, and no local variables), STD (ECMAScript and TypeScript syntax of
  parameter lists and type annotations), own measurement 2026-11-12 with
  typescript-language-server (behaviour 13, "Inferred types")
- **Feature matrix rows:** F-034, F-035

## Purpose
See what a file contains and jump to one of its declarations without scrolling.

## Behaviour
1. **Start:** `FileStructurePopup` asks the active file's language server for its symbols. The server
   receives the editor's current text, so unsaved declarations are listed. With no symbols the status
   bar says "No symbols in this file."; a failure is shown there too. [GEN, OSS, STD]
2. **Tree:** a popup just below the caret's line (moved to stay inside the window), titled with the file name, lists the symbols in
   document order, indented by nesting, each with the icon of its kind (class, method, function, field,
   variable, …) and the server's detail (e.g. a signature) dimmed. The symbol around the caret (the last
   one starting at or before it) is selected; the selection covers the whole row and keeps the detail
   readable. [GEN, OSS]
3. **Speed search:** typing filters to symbols whose name contains every typed word, ignoring case; the
   parents of a match stay visible but dimmed; the first match is selected. Backspace removes the last
   character; with an empty filter the selection returns to the symbol at the caret. [GEN]
4. **Keys:** Up/Down select with wrap-around, Enter or a click jumps to the symbol (the place is
   remembered for Back, SPEC-0071), Escape closes; focus returns to the editor. [GEN]
5. The MCP counterpart is `document_symbols` (SPEC-0024); it works on the saved file. [ADR-0008]
6. **No package row (v3):** a symbol of kind package (jdtls lists the `package` clause first) is not
   shown; its children, if any, move up one level. The reference shows the file's types and members
   only (OBS-0016 Nr. 7). [OBS-0016]

7. **Order (v4):** below each parent the symbols are ordered by kind, then by name ignoring case
   (stable for equal names): initializers, constructors, methods and functions, properties,
   fields, constants, enum members and variables, nested types (class, interface, enum, struct,
   record, …), anonymous classes, anything else. Behaviour 2's "document order" holds only within
   equal kind and name. [OSS, OBS-0016 Nr. 9]
8. **Nested types closed (v4):** a type below another symbol starts collapsed, unless the symbol at
   the caret is inside it; its row has a chevron. Right expands (or moves to the first child),
   Left collapses (or moves to the parent); a click on the chevron toggles. While the filter holds
   text every row with a match below it is expanded. [OBS-0016 Nr. 9]
9. **Filters (v4):** three toggles above the tree, each with its key, all off when the popup opens:
   - **Inherited members** (⌘F12 / Ctrl+F12 again, as in the reference): below each type of the
     file its inherited members, taken from the server's supertypes of that type
     (`typeHierarchy/supertypes`, transitively, at most 20 types, only types in project files)
     and the symbols of their files; each inherited row is grey and names its type in the detail
     (`↑ Discount`); members the type declares itself (same name) are not repeated. Enter on one
     opens its file. Asked on first use, at most 5 s; without an answer the status bar says the
     server gave no supertypes.
   - **Anonymous classes** (⌘I / Ctrl+I): when off, symbols of anonymous classes (names starting
     with `new ` or holding `{...}` / `<anonymous`) and everything below them are hidden.
   - **Lambdas** (⌘L / Ctrl+L): when off, symbols the server reports for lambdas (names holding
     `->` or starting with `lambda`) are hidden; jdtls reports none, so it changes nothing there.
   [OSS, DOC, STD, OBS-0016 Nr. 9]
10. **Filter field and matches (v4):** the typed text stands in a field below the title, with a
   search icon (not in the title); in each matching name the matched parts are drawn in the
   accent colour on the search-hit background. Typing, Backspace and the selection rules of
   behaviour 3 are unchanged. [OBS-0016 Nr. 9]
11. **MCP (v4):** `document_symbols` takes `structure: true` to return the symbols in the order of
   behaviour 7 with anonymous classes and lambdas left out (`anonymous: true`, `lambdas: true`
   keep them) and `inherited: true` to add inherited members with their `inherited_from` type and
   `path`. [ADR-0008, IMP-0301]
12. **Python (v5):** pyright and basedpyright report a function's parameters and local variables
    as its children and no signature. For a Python file IDECraft therefore reshapes the server's
    symbols once, where they are read (`insight::symbols_in`), so the popup, the Structure tool
    window, a remote project's host and `document_symbols` show the same. No such per-language
    step existed before; the only earlier one is the package row of behaviour 6, applied by each
    consumer. [OBS-0034 item 43]
    - **Left out:** below a function or method only nested functions and classes stay, with what
      is below them; every other symbol there (parameters, local variables, constants) is
      dropped. Variables of the module and of a class stay, among them the attributes a method
      assigns with `self.x = …`, which pyright lists below the class.
    - **Parameter list:** a function or method gets its parameter list as `detail`, read from
      the syntax tree of the same text at the symbol's name: `(a, b=2, *args, **kw)`. Each
      parameter is its name with its stars; a default follows as `=` and its source text with
      runs of white space folded to one space, cut after 20 characters with `…`; the bare `*`
      and `/` markers stay; annotations and the return type are left out, to keep rows short
      (own choice). A lambda bound to a name stays the variable the server reports. Where the
      text has no function at the symbol's place (a server answering for older text), the
      symbol keeps the server's detail.
    - The popup draws the detail dimmed after the name (behaviour 2), so a row reads
      `messy (a, b=2, *args, **kw)`; `document_symbols` returns it in `detail`, with and
      without `structure`.

13. **JavaScript and TypeScript (v6):** typescript-language-server reports the local variables
    of a function as its children, an arrow function bound to a name as a constant or variable,
    and an empty detail for every symbol. For a JavaScript or TypeScript file (`.js .jsx .mjs
    .cjs .ts .tsx .mts .cts`) the symbols are therefore reshaped in the same place as for
    Python (behaviour 12), from the syntax tree of the same text. The script blocks of Vue and
    Svelte files go to other servers and are not reshaped. [OBS-0028 T1]
    - **Functions:** a function declaration, a method, a constructor, a method or function
      signature (overload, interface, abstract), and a variable, constant, class field or
      object property whose value is an arrow function or a function expression. Such a
      variable or constant is reported with kind `function` (`lam` in
      `const lam = (q: number) => q + 1`); a class field or object property keeps its kind.
    - **Left out:** below a function only functions (as defined above) and classes, interfaces
      and enums stay, with what is below them; every other symbol there (local variables and
      constants, with their object members) is dropped. Callbacks the server lists as functions
      stay. Variables of the module and members of a class, interface or object stay.
    - **Parameter list:** a function gets `(p1, p2, …)` as `detail`. Each parameter is its
      name as written (a destructuring pattern with white space folded, cut after 20
      characters with `…`), `...` before a rest parameter, `?` after the name when the
      parameter is optional or has a default value, then `: ` and its type when the source
      annotates one. Accessibility and `readonly` modifiers of constructor parameters,
      decorators and default values are left out. When the source annotates the return type it
      follows the list: `(a: number, b?, ...rest: string[])`, `(): number`,
      `(x: unknown): x is string`. Type parameters are left out. A getter gets only its
      annotated type (`: number`), a setter its parameter list.
    - **Types of members and variables:** a variable, constant, class field, interface
      property or object property that is no function gets `: ` and its annotated type as
      `detail` (`: number`); without an annotation it keeps the server's detail.
    - A type is the source text of the annotation with runs of white space folded to one
      space, cut after 40 characters with `…`.
    - **Inferred types:** only annotated types are shown. The reference also shows the types
      it infers (`name(): string`, `sides: number`, `b?: number` in OBS-0028 T1). The server
      would give most of them as inlay hints (SPEC-0291), but that question is answered only
      after the project is loaded and costs far more than the symbols: measured on a 2400-line
      file 12 ms for the symbols against 445 ms for the hints with a loaded project, and on a
      freshly started server 1.2 s to 1.4 s for the symbols against a further 2.3 s to 2.5 s
      for the hints. The structure is opened to jump and must not wait for that, so
      unannotated members stay without a type (named deviation).
    - Where the text has no such declaration at the symbol's place, the symbol keeps the
      server's kind and detail.
14. **HTML (v7):** the symbols of an HTML file are its elements from IDECraft's own syntax tree,
    nested as in the document, each named with its id and classes and with the address of a
    `link`, `a`, `img` or `script` as detail (SPEC-0042 v17 behaviour 31). They have the kind
    `element`. Elements are not ordered by kind and name (behaviour 7): below each parent they
    keep the order of the document, in the popup, in the Structure tool window and in
    `document_symbols` with `structure: true`. No element starts collapsed (behaviour 8 speaks
    of types). The filter field matches the whole name, so typing `.card` or `#prices` finds
    the elements of that class or id. [OBS-0039, SPEC-0042]

## Acceptance criteria
- **AK-1** Symbols of unsaved text with their nesting (integration test with clangd).
- **AK-2** Filter keeps parents of matches; start selection at the caret (unit tests of
  `structure_model`).
- **AK-3** In a real window (`idecraft ui --selftest`, with clangd): ⌘F12 inside `main` lists the
  functions with `main` selected; typing `ca` leaves `calc`; Enter jumps to it.
- **AK-4** Use by hand: icons, nested classes, long files, Back after the jump.
- **AK-5 (v3)** A package symbol is left out and its children keep their order (unit test
  `structure_model::tests::the_package_row_is_left_out`).

- **AK-6 (v4)** Kind order and names, collapsed nested types with the caret's type open, the three
  filters and the matched ranges (unit tests `structure::tests::members_before_nested_types`,
  `structure::tests::anonymous_and_lambdas_are_filtered`, `structure_model::tests::nested_types_start_closed`,
  `structure_model::tests::matched_parts_of_a_name`); inherited members of a Java class through
  jdtls (live test `tests/java_navigation.rs`); in a real window ⌘F12 still lists `calc` and `main`
  (window self-test).
- **AK-7 (v5, behaviour 12)** Parameters and local variables of a function and of a method are
  left out, a nested function and a class inside a function stay with their own members, module
  variables and class fields stay; the parameter list with defaults, stars, the bare `*` and
  `/`, a long default cut, annotations left out, on a line with non-ASCII text before the name
  (unit tests `python_structure::tests`); with pyright, `document_symbols` on a Python file
  lists `messy` with `(a, b=2, *args, **kw)` and no children, and `Shape` with `sides`, its
  methods and the attribute `__init__` assigns (live test `tests/servers.rs`, skipped without the server; `idecraft call`).

- **AK-8 (v6, behaviour 13; OBS-0028 T1)** For TypeScript, TSX and JavaScript text: local
  variables and constants of a function, of a method and of an arrow function are left out, a
  nested function, a nested arrow function and a class inside a function stay; an arrow function
  and a function expression bound to a name are functions; parameter lists with optional,
  defaulted, rest, destructured and constructor parameters, annotated types, return types and
  type predicates; types of fields, interface properties and variables; long types cut; a line
  with non-ASCII text before the name (unit tests `script_structure::tests`); with
  typescript-language-server, the symbols of unsaved text like that of OBS-0028 T1 list
  `lam (q: number)`, `messy (a: number, b?, ...rest: string[])` without children and `Shape`
  with `area (): number`, `name ()` and `sides` (live test
  `tests/it/servers.rs::typescript_structure_without_locals`, skipped without the server;
  `idecraft call`).
- **AK-9 (v7, behaviour 14; OBS-0039 W3)** SPEC-0042 AK-31: the elements of the sample with
  depths, names and details (`html_css::tests::structure_is_nested_and_named`); the order of the
  popup leaves elements where the document has them
  (`structure::tests::elements_keep_document_order`).

## Out of scope
Inferred types in the structure of a JavaScript or TypeScript file, and the structure of the
script block of a Vue or Svelte file (v6). A Python structure without a language server (v5); ordering the popup's rows of a Python file
by name alone, as the reference appears to (OBS-0034 item 43 lists the variable `lam` before the
function `messy`; behaviour 7 puts functions before variables) (v5). The settings button of the popup, sorting by visibility, the Structure tool window, following the
caret while the popup is open, inherited members from library or JDK types (the reference lists
those of `Object` as well).

## Known deviations from the reference
The reference builds the structure from its own parsers; IDECraft shows the language server's symbols,
so what is listed depends on the server: lambdas only when the server reports them, inherited
members only from supertypes in project files.
For Python (v5) the reference shows the parameter list as part of the name; IDECraft shows it as
the dimmed detail after the name, and leaves annotations out of it.
For JavaScript and TypeScript (v6) the reference shows the signature as part of the name and
also the types it infers; IDECraft shows the signature as the dimmed detail after the name and
only the types the source states: `name ()` and `sides` where the reference shows
`name(): string` and `sides: number`, `b?` where it shows `b?: number`. A rest parameter keeps
its dots and its array type (`...rest: string[]`); the reference shows `rest: string`
(OBS-0028 T1). Whether the reference lists callbacks inside a function was not observed;
IDECraft keeps those the server reports.

v7 (HTML): the reference shows the text of a button beside it; IDECraft lists no text. All
elements share one icon. The popup and the tool window were not looked at in a window for an HTML
file; the rows were checked through `document_symbols`.
