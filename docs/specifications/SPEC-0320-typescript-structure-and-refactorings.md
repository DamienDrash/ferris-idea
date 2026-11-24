# SPEC-0320: Type hierarchy, usage counts, gutter marks and class refactorings for JavaScript and TypeScript

- **Version:** 8 · **Date:** 2026-11-13 (v8: a usage count is the number of places Find Usages lists, without the members that implement or override the counted one, behaviour 5a, AK-18, OBS-0028 T8); 2026-11-13 (v7: Ferris IDEA's own Extract Variable where the server offers no extraction, TypeScript 7's `tsc --lsp`; behaviours 23–27, AK-17, OBS-0028 T4); 2026-11-11 (v6: without `tsconfig.json`/`jsconfig.json` the structure comes from the syntax trees also with a server, behaviour 21, found with a real server in PR 525; v5: script files without the language server — hierarchy, marks and inheritors from the project's syntax trees; members count implementations and overrides, behaviours 21–22, F-036; v2: gutter marks for Java, behaviours 17–18; v3: gutter marks for Kotlin, behaviour 19; v4: marks after an edit, behaviour 20)
- **Sources:** OBS-0028 items 2, 6, 9 (L2, L6, L10); STD (LSP 3.17 `textDocument/definition`,
  `textDocument/references`, `textDocument/documentSymbol`, `textDocument/hover`, `textDocument/codeAction`;
  typescript-language-server has no type hierarchy provider), GEN (what Extract Interface, Extract
  Superclass, Extract Field and Pull Members Up do in object-oriented code; TypeScript's class and
  interface syntax), SPEC-0082 v5 (hierarchy window), SPEC-0292 (usage counts), SPEC-0095 (refactoring
  shortcuts), SPEC-0193 (undo group), SPEC-0042 v10 (server without its syntax server), GEN v5
  (ECMAScript and TypeScript module syntax: named, default and namespace imports, `export … from`,
  `export *`; how TypeScript finds a relative module: the extensions, `.js` written for a `.ts` file,
  a folder's `index`), SPEC-0292 v6 behaviour 9 (the same counts for Kotlin), SPEC-0082 v6 and this
  spec's v3 (the Kotlin way: answers from the project's syntax trees)
- **Feature matrix rows:** F-036

## Purpose
In JavaScript and TypeScript files see how classes and interfaces relate, how often a declaration is
used and what a member overrides, and restructure classes without editing by hand.

## Behaviour
**Files.** Script files are `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`. Declarations and
their `extends`/`implements` clauses are read from the syntax tree; which declaration a name means comes
from the language server. No text search decides what a name refers to. [GEN, STD]

**Type hierarchy.**
1. `TypeHierarchy` (⌃H) and MCP `type_hierarchy` work in script files at the name of a class or interface,
   or anywhere inside one; the window and the three directions are those of SPEC-0082 v5 behaviour 13. The
   type asked for is marked `*`. Without a type at the place: "no type". [OBS-0028 item 6]
2. Supertypes of a type: the declarations the server gives as definition of each name in its
   `extends`/`implements` clauses. One outside the project (a library, `node_modules`) is a leaf. A name
   the server cannot resolve is left out. [STD]
3. Direct subtypes of a type: the server's references to its name that stand in the `extends`/`implements`
   clause of a class or interface of the project. So renamed imports count and a name in a type argument
   does not. [STD]
4. The class direction goes up through `extends` of classes only; an interface starts at itself. At most
   500 nodes; a repeated type is marked as recursive and not expanded (SPEC-0082). [GEN]

**Usage counts (extends SPEC-0292 for script files).**
5. Counted: at file level classes, interfaces, enums, type aliases, functions, variables and constants;
   in classes, interfaces and enums their methods, properties, fields, accessors and enum members; not
   constructors and not anonymous symbols. At most 200 per file; when the time is up the rest shows
   nothing. [OBS-0028 item 2]
   5a. **What a count counts (v8, OBS-0028 T8):** the places Find Usages lists for the declaration
   (SPEC-0293 v4 behaviours 3 and 16; one function, `find_usages::usages_of`, gives both, so they
   cannot drift apart): in project files, each once, not the declaration itself, and not the
   declarations of members that implement or override it — those are counted as implementations or
   overrides (behaviour 22). `area(): number;` of `interface Shape`, implemented by `abstract class
   Base` and in `Circle` and `Square`, called once in `Base.describe`, reads `1 usage · 3
   implementations`, `abstract area()` in `Base` `1 usage · 2 implementations`; before v8 they read
   `4 usages …` because the server's references name the three declarations. Types are not
   affected: `Shape` reads `1 usage · 3 inheritors` (its `implements Shape`), as Find Usages lists.
   The servers differ in what they name: typescript-language-server counts the two imports of
   `Base` as usages, TypeScript 7's `tsc --lsp` does not (`4 usages` against `2 usages`); count and
   list agree with either. Java, Kotlin and Python counts are not changed (SPEC-0292; their servers
   were not checked here). [OBS-0028, GEN]
6. The label: `no usages` for none (only in script files; other languages keep leaving it out), `1 usage`,
   `N usages`. A type with subtypes adds ` · 1 inheritor` / ` · N inheritors`: all project subtypes, direct and
   indirect. Two declarations on one line: the first one shows. [OBS-0028 item 2]
7. MCP `usage_counts` returns the same list with `inheritors`. [ADR-0008]

**Gutter marks.**
8. On the line of a member or type a mark in its own column left of the run marks: ↑ the member overrides
   or implements something in a project supertype, ↓ it is overridden or implemented in a project
   subtype, or the type has subtypes, ↕ both. Static and private members and constructors have none;
   a getter and a setter match by name. Implementing means the upper declaration is in an interface or
   abstract. [OBS-0028 item 2, GEN]
9. A tooltip names what the mark means. A click goes to the single target, several open the usual
   choice of places. The marks are computed after open and save like the counts, independent of the
   usage setting, move with their lines and are dropped when their line is edited. No column is reserved
   in a file without marks. [GEN]
10. MCP `declaration_marks` (path): line, character, name, kind (`overrides`, `implements`, `overridden`,
    `implemented`, `inherited`) and targets. [ADR-0008]

**Extract Field.**
11. `IntroduceField` (⌥⌘F), `Refactor This` and `extract_field` of `code_actions` use the server's action
    for a field; the selection widens like Extract Constant (SPEC-0095 v2). TypeScript offers a readonly
    field in the class for an expression that needs no local value; otherwise the usual message
    says it cannot. The client answers the server's request to start a rename after an extraction with
    nothing; the edit stands without it. [OBS-0028 item 9, STD]

**Extract Interface, Extract Superclass, Pull Members Up.** Actions `ExtractInterface`, `ExtractSuperclass`,
`MembersPullUp` in the Refactor menu; MCP `extract_interface`, `extract_superclass`, `pull_members_up` with
`dry_run` (default true) and `class_members` listing what can be chosen. In the window a name field
(extracting), a target choice (several supertypes), then a list with check boxes. Applied as one undo step
(SPEC-0193). They work on the class or interface at or around the caret. In the window pulling always
moves the implementation; `abstract` is an MCP option. [OBS-0028 item 9, GEN]

12. **Extract Interface (TypeScript only; JavaScript has no interfaces):** offers public non-static methods,
    fields and accessors, all checked. Creates the interface directly before the type in the same file,
    exported if the type is, with the signatures of the chosen members; parameter defaults become optional
    parameters, parameter properties lose their modifiers. A missing type comes from the server's hover.
    The class gets `implements Name` (appended to an existing list); an interface gets `extends Name`. Members
    stay. [GEN, STD]
13. **Extract Superclass (classes):** offers the movable members of behaviour 14, none checked. Creates
    `class Name` directly before the class, exported if the class is; abstract if the class is. It inherits
    what the class inherited, and the class then extends it. Chosen members move there. A constructor of a
    class that had no superclass gets `super();` as its first statement. [GEN]
14. **Movable members:** methods, fields and accessors, static ones too, not constructors and not parameter
    properties. A moved `private` member that remaining code uses becomes `protected`. [GEN]
15. **Pull Members Up:** targets are the direct supertypes in the project. Into a class the members move to
    the end of its body, re-indented; with `abstract` (target must be an abstract class) an `abstract`
    signature goes up and the implementation stays. Into an interface only the signature goes up and the
    member stays. [GEN]
16. **Refused, changing nothing, naming each reason:** the name is not an identifier or already declared in
    the file; no member chosen; a moved member uses `this.x` of a member that neither moves nor exists in the
    target, or `super` when the class above changes; uses a type parameter of its class; a target
    already declares the member; moving to another file a member that uses an import or a local top-level
    declaration of its file that the target's file does not have; a target outside the project. [GEN]

**Java (v2).**
17. In Java files the gutter marks of behaviours 8–9 use jdtls' type hierarchy and the syntax tree of each project
    file: types and methods, not constructors, no `private` or `static` methods; a method matches by name and parameter
    types; `implements` when the upper declaration is in an interface or abstract. [OBS-0027 item 4, STD]
18. MCP `declaration_marks` works on Java files. Remote projects have no marks yet. [ADR-0008]

19. **Kotlin (v3):** `.kt` files get the marks of behaviours 8–10 with the same glyphs, clicks and MCP tool
    (`declaration_marks`), computed from the project's syntax trees as in SPEC-0082 behaviour 17 (no language
    server). A type with project subtypes is *inherited* (targets: every subtype, nearest first). A function or
    property — also a `val`/`var` of the primary constructor — that a project supertype declares *implements*
    it when the nearest such supertype is an interface or declares it abstract, else *overrides* it; one that
    can be overridden and that subtypes declare is *implemented* when it is abstract, else *overridden*.
    Private members take no part. Members match by kind, name and number of parameters. [OBS-0035 item 5]

20. **Marks after an edit (v4):** [OBS-0035 window check: after Implement Members added a member, the mark
    of another override in the class was gone]
    - An insertion that only adds whole lines (SPEC-0083 v4 behaviour 5a) changes no existing line: the
      marks above the new lines stay, also the mark of the line the lines were appended to, and the marks
      below move down. Before v4 the mark of the line appended to was dropped, and came back only with
      the next save.
    - Any other edit drops the mark of a line it touches until the marks are computed again (behaviour 9).
    - In Kotlin files the marks are computed again after every edit of the file, from the editor's text
      and the project's syntax trees, one computation at a time; they need no language server, also not
      the server pool. Java and script files ask their server again when the file is saved, as before.

**Without the language server (v5).** [F-036; OBS-0028 L2, L6 were fixed with the server only]
21. **Syntax trees of the project.** When typescript-language-server is not installed, does not start or
    fails while answering, `TypeHierarchy`, the gutter marks and the MCP tools `type_hierarchy` and
    `declaration_marks` answer for script files from the project's syntax trees instead of nothing. A
    name in `extends`/`implements` means, in this order: a class or interface of the same file with that
    name; else the import that binds it — `import { X }`, `import { X as Y }`, `import X` (the module's
    `export default class …` or `export default Name`) and, for `ns.X`, `import * as ns`. A relative
    module (`./`, `../`) is the file as written, with one of the script extensions (`.ts`, `.tsx`,
    `.d.ts`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`), a `.ts`/`.tsx` file for a written `.js`
    (`.mts` for `.mjs`, `.cts` for `.cjs`), or its folder's `index.ts`/`.tsx`/`.js`/`.jsx`. In that file
    the name is an exported class or interface, a name of `export { a as b }`, or is followed through
    `export { X } from`, `export { X as Y } from` and `export * from` (at most eight steps). A name from
    a package (a module not starting with `.`) or a global name (`Error`) is a leaf placed where it is
    written. A relative module that is not found, or a name it does not export, is left out, as a name
    the server cannot resolve (behaviour 2). Subtypes are the types of project files whose names resolve
    so to the type. Project files are the script files below the project folder except in
    `node_modules`, `dist`, `build`, `out`, `coverage`, `target` and folders starting with `.`, at most
    5000; only files that contain the type's name, or import a default export, are parsed for this.
    The rest of behaviours 1–4 and 8–10 is unchanged: the same nodes, glyphs, targets and limits. [GEN]
    **(v6)** The same holds with a running server when no `tsconfig.json` or `jsconfig.json` lies in
    the file's folder or above it, up to the project folder: the server then builds its project from
    the open files only and answers without the subtypes in other files (an empty list instead of an
    error). Supertypes and subtypes then come from the syntax trees; usages still from the server.
    [STD, found with typescript-language-server 5.3.0 and TypeScript 5.6.3]
22. **What is below a member.** Usage counts of script files (behaviour 5) give a method, field or
    accessor that can be overridden (behaviour 8) the members of all project subtypes that declare it
    again by name: below a member of an interface or an `abstract` member these are implementations
    (`2 usages · 2 implementations`, kind `abstract method` or `abstract property` in MCP
    `usage_counts`), below another member overrides (`1 usage · 1 override`), counted in `inheritors`
    as for Kotlin (SPEC-0292 v6 behaviour 9). Subtypes come from the server, or from behaviour 21 when
    there is none. The usages themselves still need the server: without one, script files show no
    counts, as Java and Kotlin files. [OBS-0035 L1 for Kotlin, GEN]

**Extract Variable without the server's help (v7, OBS-0028 T4).**

23. **When:** in a `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs` or `.cjs` file, Extract Variable
    (and Refactor This) use Ferris IDEA's own action "Extract variable" when the language server offers no
    matching action there or cannot be asked. typescript-language-server offers one (its "Extract to
    constant in enclosing scope", SPEC-0095 v6 behaviour 2); TypeScript 7's own server (`tsc --lsp`,
    SPEC-0042 v12 behaviour 21) offers no refactoring at all, checked with typescript-go 7.0.2. A bare
    caret is widened as in SPEC-0095 behaviour 6: the expressions around it up to the statement, no
    plain name, no callee. Extract Constant and Extract Method stay the server's. [OBS, GEN]
24. **A value:** the selection is one complete expression (surrounding spaces ignored; `x` in `(x)`
    is not `(x)`). Refused: the target of an assignment or of `++`/`--`, a declared name, a parameter
    or pattern, a key of an object literal, an imported or exported name, a decorator, a method being
    called (`obj.run` in `obj.run()`, which would lose its `this`), the name of the function called,
    the operand of `delete`, a plain name under `typeof` (it may not be declared), an expression that
    assigns inside or contains `yield`, `super`. [STD]
25. **Evaluated at the same time, once, in the same scope:** the new line runs before everything
    else of its statement. Refused, each with its reason: the concise body of an arrow function, a
    default value of a parameter, the right side of `&&`, `||`, `??` and of `&&=`, `||=`, `??=`, a
    branch of `?:` (the condition is allowed), anything after `?.` in its chain, a loop's condition,
    the head of a `for` loop (the iterable of `for … of`/`in` is allowed), the condition of
    `else if`, a `case` value, a statement directly in a `case` or the body of `if`, `else`, a loop
    or a label without braces, a class field, a decorator, a statement that does not start its line,
    an expression reading a name the same statement declares or assigns (`const a = 1, b = a + 1`).
    The order of evaluation: JavaScript evaluates left to right, also an assignment's target before
    its right side; when a part evaluated before the selection holds a call, `new`, `await`,
    `yield`, an assignment or `++`/`--` (not inside a function defined there) and the selection
    itself calls, constructs or awaits, it is refused, naming that part ("it would then run before
    `first()` …"). As for Python (SPEC-0330 v2 behaviour 8), an expression without a call after a
    call that changes what it reads is not detected. [STD]
26. **The edit:** `const <name> = <expression>;` in a line of its own before the statement, with
    the statement's indentation and line ending, always with `;` (a next line starting with `(` or
    `[` would otherwise continue the declaration); the expression is replaced by the name. A whole
    expression statement becomes the declaration itself. A comma expression is put in parentheses.
    The name: a comparison or logical expression `condition`, a call or `new` its function's or
    class's name (`getTotal` → `total`), a property its name, an array `items`, a string `text`, a
    function `callback`, else `value`; a name taken anywhere in the file, a keyword or a global such
    as `default` or `console` gets a number (`total1`). The name popup of SPEC-0095 v3 behaviour 8
    opens at it. [GEN]
27. **Refusals** change nothing and say "Cannot extract `…` into a variable: <reason>." in the
    status bar and in MCP `code_actions` `message`. A file with syntax errors at the place is not
    changed. [GEN]

## Acceptance criteria
- **AK-1** With typescript-language-server over two files: `Base` (abstract) with `Circle`, `Square` and an
  interface `Shape`: class direction from inside `Circle` gives `Base › * Circle`; subtypes of `Base` name both;
  supertypes of `Circle` name `Base` and `Shape`; a renamed import still counts.
- **AK-2** Usage counts with the server: functions, methods and fields counted, `no usages` for an unused one,
  ` · 2 inheritors` at `Base`.
- **AK-3** Marks with the server: `area` in `Circle` implements, in abstract `Base` is implemented, an override
  of a concrete method is `overrides`/`overridden`; private and static have none.
- **AK-4** Extract Field with the server yields a readonly field and a `this.` use.
- **AK-5** Extract Interface: signature rules, heritage in all three shapes, hover types with the server, the
  result has no type errors.
- **AK-6** Extract Superclass: moved members, `super();`, existing `extends`, private to protected.
- **AK-7** Pull Members Up across files with the server: body, signature for an interface, abstract, re-indent.
- **AK-8** Each refusal of behaviour 16 changes nothing and names the reason.
- **AK-9** Gutter mark model: glyphs, moving with lines, dropping on edit. The MCP tool list names the new tools.
- **AK-10 (v2)** With a real jdtls over an abstract class and a subclass: `implemented` and `implements` with their targets, `inherited` on the type, a private method without a mark.

- **AK-11 (v3)** On the Kotlin shapes of OBS-0035: `Animal`, `Pet` and sealed `Shape` are inherited, abstract
  `sound()` and `Pet.owner` are implemented, both `override fun sound()` and `Cat`'s constructor property
  `owner` implement, an open function nobody overrides has no mark (unit test
  `kotlin_structure::tests::gutter_marks`; through the entry point on files on disk
  `crates/languages/tests/it/kotlin_structure.rs`; MCP `declaration_marks` on a `.kt` file,
  `crates/mcp/tests/it/protocol.rs`).
- **AK-12 (v4)** Mark model: lines appended to a marked line keep its mark and move the marks below; lines
  put in at the start of a line move that line's mark; an insertion inside a line, without a line break or
  not ending one is an ordinary edit (`decl_marks_model::tests::whole_lines_put_in_keep_the_marks_beside_them`).
- **AK-13 (v4)** In a window without a language server
  (`crates/workbench/tests/it/jvm_editor_rest.rs::kotlin_gutter_marks_follow_an_inserted_member`): a Kotlin
  file shows ↑ at both `override fun sound()`; after Implement Members adds `owner` below `Dog.sound()`,
  that mark is still there at once, and the recomputed marks show ↑ at the new `owner` and at
  `Cat.sound()` on its new line.

- **AK-14 (v5)** Without a server (`ts_structure::tests::{without_a_server_names_are_resolved_through_the_project,
  without_a_server_the_type_hierarchy_has_all_directions, without_a_server_the_gutter_marks_are_found,
  reads_imports_and_exports}`): a re-export through `index.ts`, a namespace import, a default export
  imported under another name, a `.js` written for a `.ts` file; `react` and `Error` are leaves where
  written, a missing relative module is left out; `node_modules` and `dist` hold no subtypes; marks
  `inherited`, `implemented`, `overridden`, `implements`, `overrides`; private and static members have
  none. Through the entry points of window and MCP, with or without a server
  (`crates/languages/tests/it/ts_structure.rs::structure_through_the_entry_points_with_or_without_a_server`).
- **AK-15 (v5)** `ts_structure::tests::members_count_their_implementations_and_overrides`: abstract
  `area` 2 implementations, an interface member 1, `describe` 1 override, private and static 0; the labels.
  With a real server (`usage_counts_with_members_and_inheritors`): the interface's `area` ends with
  ` · 3 implementations` (`Base`, `Circle`, `Square`), `Base`'s abstract `area` with ` · 2 implementations`,
  `describe` with ` · 1 override`.
- **AK-16 (v6)** `ts_structure::tests::project_config_is_looked_for_up_to_the_root`; with a real server
  in a project without `tsconfig.json`, subtypes of `Base` in another file
  (`structure_through_the_entry_points_with_or_without_a_server`). In the window: to be seen on the Mac.
- **AK-17 (v7, behaviours 23–27)** Unit tests `ts_extract::tests::{the_observed_condition,
  the_cases_of_the_task_in_every_dialect, refusals_name_their_reason,
  allowed_neighbours_of_the_refusals, names_are_unique_and_no_keywords,
  a_bare_caret_offers_the_expressions_around_it}`: `x>3` of OBS-0028 item 4 gives `const condition
  = x>3;` and `if(condition) return x`; `a+b*2`, `[1,2,3]`, a whole statement, `await`, `new`,
  a comma expression and JSX in `.ts`, `.tsx`, `.js`, `.jsx` and `.mts`; 33 refusals with their
  reason and 13 allowed neighbours that parse afterwards. Live (`crates/languages/tests/it/
  ts_extract.rs::extract_variable_works_with_every_typescript_server`): Extract Variable on `x>3`
  works with TypeScript 7's `tsc --lsp` (own action) and with typescript-language-server (its
  action). MCP: `protocol.rs::typescript_extract_variable_tool`.

- **AK-18 (v8, behaviour 5a)** `find_usages::tests::usages_of_keep_calls_and_drop_implementations`
  (the declaration itself, implementing members, places outside the project and repeats dropped;
  the unsaved text read; other languages unchanged). Live with TypeScript 7's `tsc --lsp` and with
  typescript-language-server (`crates/languages/tests/it/ts_usages.rs::usage_counts_say_what_find_usages_lists`):
  the three labels above, and every count equal to the length of Find Usages' list at its place.
  MCP `usage_counts` (`protocol.rs::usage_counts_of_an_interface_method_leave_out_its_implementations`).
## Out of scope
Method Hierarchy, Override/Implement Methods and Go to Super Method in script files; class expressions;
declaration merging beyond the first declaration; moving the extracted type into a new file; window checks
(hand tests 60 and 67).

## Known deviations from the reference
The reference was only seen listing these refactorings; its dialogs are not rebuilt. An extracted type stays
in the file of its class. A conflict refuses rather than offering to continue anyway.

**Window check 2026-11-09** (`main` @ 3b3cc1e0, agent run with real input, OBS-0028): ⌃H on a class (Base > Circle, tabs Class, Supertypes, Subtypes), usage counts at class, constructor and method, the ↑ mark at `area`, and the three Refactor menu entries were seen. Not exercised in the window: applying Extract Interface, Extract Superclass and Pull Members Up; clicking the mark and its tooltip; Extract Field. Hand tests 60 and 67 remain open.

v5: without the server, names are resolved by the rules of behaviour 21 only: `paths` and `baseUrl`
of `tsconfig.json`, package `exports`, imports from other packages of a workspace, `require`, `export =`,
classes assigned to variables and mixins are not followed (such a supertype is a leaf or left out).
Two types of one name in one file are not told apart. Behaviour 22 matches members by name, as the
marks do (behaviour 8); overloads are one member.

v3 (OBS-0035): Kotlin marks know the project's `.kt` files only: a member that overrides one of a library
type (`toString`, `compareTo`, `run`) has no mark, where the reference shows one.
