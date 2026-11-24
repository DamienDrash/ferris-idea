# SPEC-0122: Java and Kotlin in the editor — run markers, Optimize Imports, Generate

- **Version:** 10 · **Date:** 2026-11-23 (v10: Generate also lists Override, Implement and
  Delegate Methods, Test… and Copyright, behaviours 3d and 3e, MCP `create_test`, behaviour 4a,
  AK-25 and AK-26; OBS-0119 K2); 2026-11-13 (v9: the questions behind the name and behind the Inline
  Method variants have a limit, after which the server's name and the server's one action stay,
  behaviours 16 and 17, AK-24; window of 2026-11-13); 2026-11-13 (v8: Extract Constant and Extract Variable name what they
  introduce after its place, Inline Method offers its variants; OBS-0027 J1, J2); 2026-11-11 (v2: Generate puts the code where the caret is; hand test Nr. 49;
  v3: Generate offers only what applies, Override/Implement Methods, Surround With, Lombok; OBS-0027;
  v4: field and template choice for Generate, Delegate Methods, import hint; OBS-0027 L9, L17, L22;
  v5: the lists open at the caret and filter while typing, the import hint imports at once, the hint
  outside a class passes; OBS-0027 L10, L36, L37 and the side finding of the second window re-check;
  v6: Override and Implement Members for Kotlin, OBS-0035 L4; v7: Implement Methods lists default
  methods, the rows carry return types, OBS-0027 L30)
- **Sources:** GEN (IDEs show a run icon in the gutter next to `main` and tests, run a single test
  method, optimize imports and generate constructors, accessors, `equals`/`hashCode` and
  `toString`; F-037), OSS (action ids `OptimizeImports`, `Generate` with their shortcuts in the
  ported keymaps), STD (JUnit 4/5 annotations; Maven Surefire `-Dtest=Class#method`; Gradle
  `--tests pkg.Class.method`; Java and Kotlin import rules: `java.lang` and the own package need
  no import, Kotlin operator and delegate conventions), SPEC-0097 (run and debug the file),
  SPEC-0088 (test tree), OBS-0027 (side by side with the reference, Maven project with Lombok),
  jdtls protocol extensions `java/listOverridableMethods`, `java/addOverridableMethods`,
  `java/checkDelegateMethodsStatus` and `java/generateDelegateMethods` (OSS, EPL-2.0,
  only the request names and their JSON shapes are used), jdtls's diagnostic message for unresolved
  names and the title of its import quick fix (seen while probing jdtls 1.64); v8: STD
  (`textDocument/signatureHelp`, `textDocument/definition`, `textDocument/references`, code
  actions), jdtls's titles `Extract to constant`, `Extract to local variable` and `Inline Method`
  and what its Inline Method does at a declaration and at a call (seen while probing jdtls
  1.64.0), SPEC-0095 (the refactoring shortcuts this extends)
- **Feature matrix rows:** F-037

## Purpose
The everyday Java and Kotlin editing moves — run what is under the cursor, keep imports tidy,
write the boilerplate — without a language server.

## Behaviour
1. **Run markers:** a green ▶ sits in a gutter column (shown only in files that have markers) on:
   a top-level Java class with `static void main` and on that method; a top-level Kotlin
   `fun main`; a top-level class with test methods (annotations `Test`, `ParameterizedTest`,
   `RepeatedTest`, `TestFactory`, `TestTemplate`, any package) and on each test method. Nested
   classes have none. Markers follow edits. A click opens a menu: "Run 'Name'", plus
   "Debug 'Name'" for a `main`. A class runs as in SPEC-0097 behaviour 3; a test method runs
   alone (Maven `-Dtest=Class#method`, Gradle `--tests pkg.Class.method`) with the test tree,
   named `Class.method`. `RunClass` (⌃⇧R) with the caret inside a test method runs only that
   method. Tests outside a Maven or Gradle project say they need the build file. [GEN, STD]
2. **Optimize Imports:** `OptimizeImports` (⌃⌥O) in a Java or Kotlin file removes unused
   single-name imports (a name counts as used anywhere in the code or in `{@link}`, `@see`,
   `@throws` and KDoc `[...]` references), duplicates, `java.lang` (Java) and imports of the file's
   own package; wildcard imports stay, Kotlin convention imports (`getValue`, `plus`, `invoke`, …)
   stay because syntax uses them. The rest is sorted in the reference's default layout — Java:
   other imports, blank line, `javax.*` and `java.*`, blank line, static imports; Kotlin: others,
   `java.*`, `javax.*`, `kotlin.*`, aliases. With all imports gone, their blank line goes too.
   Comments between imports stop it with a message. One undo step; the caret stays on its text;
   the status bar says how many imports went. [GEN, OSS, STD]
3. **Generate:** `Generate` (⌘N on macOS, Alt+Insert elsewhere) in a Java class offers
   Constructor, Getter, Setter, Getter and Setter, `equals()` and `hashCode()`, `toString()` for its
   non-static fields: the constructor takes every field except initialized finals; getters are
   `getX`/`isX` for `boolean` (a field already named `isX` keeps its name); setters only for
   non-final fields; methods that exist are not made twice, an existing constructor with the same
   parameters, `equals`/`hashCode` or `toString` is reported instead. `equals` uses
   `getClass()`, `==` for primitives, `Float`/`Double.compare`, `Arrays.equals` for arrays and
   `Objects.equals` otherwise; `hashCode` uses `Objects.hash` and `Arrays.hashCode`; `toString` is
   `Name{a=…, s='…'}` with `Arrays.toString` for arrays. Code goes where the caret is (v2): after
   the member the caret is in, or after the last member before the caret, with a blank line before
   each method and before a member that follows; with no member before the caret it goes before
   the class's closing brace. The members' indentation is kept; `java.util.Objects`
   and `Arrays` are imported when needed. One undo step. [GEN]
3a. **Only what applies (v3):** the Generate list shows Constructor and `toString()` always,
   `equals()` and `hashCode()` when the class has instance fields, Getter, Setter and Getter and
   Setter only while a field still lacks them (a final field gets no setter; the pair only when
   setters are possible). A class without fields, or the caret inside a method of one, offers just
   Constructor and `toString()`. [OBS-0027 L8]
3b. **Choosing fields and a template (v4):** picking an entry in a Java class opens a list of the fields
   with check boxes, then a list of templates (Space toggles a row, ⌘A checks all, Enter goes on,
   Escape cancels). Listed are the instance fields the entry can use; accessors only for fields that
   still lack one. Checked at first: for Constructor only the `final` fields, a first row "(no fields)"
   chooses the constructor without parameters; for every other entry all fields. Without fields to list
   the list is skipped. Templates, the usual one first: accessors `getX`/`setX` or fluent (`x()` and
   `x(value)` returning `this`); `equals`/`hashCode` with `getClass()` or `instanceof`, each directly on
   the fields or through the class's getters where it has them (the other fields stay direct);
   `toString` by concatenation or with a `StringBuilder`, each with or without `@Override`. Constructor has
   one template and asks nothing. The generated code is otherwise behaviour 3's. Kotlin keeps its menu
   (SPEC-0132). [GEN, OBS-0027 L9]
3c. **The hint outside a class passes (v5):** when Generate cannot start where the caret is (not in a
   class, or not a Java or Kotlin file), the status bar says so as a passing hint (SPEC-0070 v10
   behaviour 4b): it is gone as soon as the caret moves, another tab becomes active or one of the lists
   of behaviour 13 opens, and it does not come back. [OBS-0027 second window re-check, GEN]
3d. **The rest of Generate (v10):** after the kinds of behaviour 3a the Java list goes on, in the
   reference's order, with Override Methods… (always: every class can override `Object`'s
   methods), Implement Methods… (when the class's declaration extends or implements a type, or is
   abstract), Delegate Methods… (when the class has instance fields), Test… (unless the file is
   named as a test, SPEC-0071 behaviour 15) and Copyright (when the project has a copyright
   profile, SPEC-0155 v5 behaviour 11). Each row runs the action of the same name: the choosers of
   behaviours 6 and 10, Test… of behaviour 3e, and Update Copyright for the file, which writes or
   updates the notice as one undo step. A row is shown only where its action can start, so the
   list never offers what would only answer with a refusal. Before v10 a class without fields
   offered only Constructor and `toString()` (OBS-0119 K2). [OBS-0119, GEN]
3e. **Test… (v10):** opens the test class of the class in the editor: the project's own, found by
   its name among the project's files as Go to Test finds it (`XTest`, `XTests`), or else a file of
   that name already in the test folder. Without one it creates `XTest` in the test folder — beside
   `src/main/<language>/<package>` it is `src/test/<language>/<package>` (also in a module), beside
   a top-level `src/<package>` it is `test/<package>`, otherwise the class's own folder — with the
   class's package and an empty class body (`public` only when the build file names JUnit 4 and not
   JUnit 5, since JUnit 4 needs it), never over an existing file and never through a link out of
   the project; the new file goes into the Local History labelled "Create Test", opens, and the
   caret stands on the empty line of its body. The status bar says whether it was opened or
   created. Kotlin classes get a `.kt` test the same way. The reference's dialog for choosing a
   framework, a superclass and methods to test is not offered: the empty class is what that dialog
   writes by default. [GEN, OSS (SPEC-0071 naming rules)]
4. **MCP:** `run_markers` (markers with line and the run configuration each starts),
   `optimize_imports` and `generate_code` (`kind`, `line`; both `dry_run` default true and refuse
   to write a file that changed meanwhile). [ADR-0008]
4a. **MCP (v10):** `create_test` (`path`, `dry_run` default true) returns where the test class of
   behaviour 3e is, whether it exists, its class name and the text of a new one; with
   `dry_run=false` it writes a new class (the `write` right), never over a file. Override,
   Implement and Delegate Methods are `override_methods` and `delegate_methods`, Copyright is
   `update_copyright`. [ADR-0008]
6. **Override and Implement Methods (v3):** `OverrideMethods` (⌃O) and `ImplementMethods` (⌃I) in a
   Java class ask jdtls which methods the class can override and which it must implement. A chooser
   with check boxes lists them grouped under their declaring type (a heading row per type, in the
   server's order: superclass, interfaces, `java.lang.Object`); Implement lists only the methods still
   to implement and checks all of them, Override lists all and checks the first. Space toggles a row,
   ⌘A checks all, Enter or a double-click adds the checked methods (the selected one when none is
   checked) through jdtls with `@Override` and a default body, as one undo step; Escape cancels.
   Without anything to list the status bar says so. Also in the Code menu. [GEN, OSS, OBS-0027 L1, L2]
6a. **Default methods (v7):** Implement Methods lists, besides the methods still to implement, the
   `default` methods of the class's interfaces, under their interface and not checked, so they can be
   chosen to be overridden as well; checked are only the methods still to implement. Kotlin's
   *Implement Members* (behaviour 15) is unchanged. [OBS-0027 item 3 (L30)]
6b. **Return types (v7):** a row of either chooser reads `name(parameter types): return type`, the
   return type as the declaring type's source writes it (`void` included). jdtls does not send return
   types, so Ferris IDEA reads them from the source of the declaring type in the project: the open file
   (as the editor holds it), else the `.java` file below the project folder whose `package` and type
   match the qualified name (a nested type in its outer type's file); folders called `target`,
   `build`, `out`, `bin`, `node_modules` and hidden ones are left out, at most 5000 files. A method is
   matched by name and the simple names of its parameter types. A type without a source in the
   project (`java.lang.Object`, a library) keeps the row without a return type. MCP
   `override_methods` lists `return_type` where known. [OBS-0027 item 3 (L30), GEN]
7. **Surround With (v3):** `SurroundWith` (⌥⌘T) in Java offers if, if / else, while, do / while, for,
   try / catch, try / finally, try / catch / finally, synchronized, Runnable and { }. The chosen
   wrapper encloses the whole lines of the selection (or the caret's line), which must be complete
   statements of one block; they move one indentation step in; the caret goes into the empty
   condition (or the new block). A local variable declared in the lines and used after them is
   declared before the construct (without `final`) and only assigned inside, so the code still
   compiles; a `var` there is refused with a hint. One undo step. Also in the Code menu. [GEN,
   OBS-0027 L6]
8. **MCP (v3):** `override_methods` (list, or `add` with names; `dry_run` default true) and
   `surround_with` (`start_line`, `end_line`, `kind`; `dry_run` default true); writing needs the
   write right and refuses a file that changed meanwhile. [ADR-0008]
9. **Lombok (v3):** when `pom.xml`, `build.gradle` or `build.gradle.kts` names Lombok and its jar is in
   the local Maven repository or Gradle's cache (the version written in the build file, else the
   newest), jdtls starts with that jar as Java agent, so `@Data`, `@Getter` and friends produce no
   false errors and their members complete. Not in safe mode, and never downloaded (IMP-0381).
   [GEN, OBS-0027 L7]

10. **Delegate Methods (v4):** `DelegateMethods` (Code menu; no default shortcut, as in the keymaps) asks jdtls
   which fields of the class at the caret can forward to which methods. With several fields a list
   picks one, then a list with check boxes picks the methods (Enter takes the checked ones, or the
   selected one when none is checked) and adds them through jdtls as one undo step. Methods every
   object has (from `java.lang.Object`: `equals`, `hashCode`, `toString`, …) are not offered, because
   forwarding them silently changes what the object is. Without a field the status bar says so.
   [OBS-0027 L22, OSS]
11. **Import hint (v4):** in a Java file, when the caret's line has a name the server reports as
    unresolved and the server offers to import it, a small hint stands above the name: the
    fully qualified class and the Show Context Actions shortcut, or the simple name and how many
    classes there are. The hint only announces; the existing Show Context Actions list adds the import,
    nothing changes by itself. It goes away when the caret leaves the line, when the problem is gone,
    while another popup is open; Escape hides it until the caret leaves the line or the problem is gone.
    Not in remote projects. [OBS-0027 L17, GEN]
11a. **Importing from the hint (v5):** while the hint is shown, Show Context Actions acts on it instead
    of opening its list: with one class the import is added at once through the server's import action
    (one undo step, the status bar names the action); with several classes a list of the fully qualified
    names opens at the caret (behaviours 13 and 14) and the chosen one is imported. While the hint is not
    shown (caret on another line, hidden with Escape, another popup open, no class to import) the
    shortcut opens the context actions as before. Still nothing is imported without the shortcut.
    [OBS-0027 L37, GEN]
12. **MCP (v4):** `generate_choices` (what applies at a line with its fields, default selection and templates;
    reads only); `generate_code` takes `fields`, `template`, `override_annotation`; `delegate_methods`
    (list, or `field` and `add`; `dry_run` default true); `import_hint` (unresolved names of a file with
    their position and candidates; reads only, adding goes through `apply_code_action`). [ADR-0008]
13. **Lists at the caret (v5):** the Generate list (Java and Kotlin), the field and template lists of
    behaviour 3b, the chooser of Override and Implement Methods, the field list and the method list of
    Delegate Methods and the class list of behaviour 11a open at the primary caret, as the context
    actions list does: the list's left edge at the caret, its top 2 px under the caret's line. When the
    list does not fit under the line but fits over it, it stands over the line (its bottom 2 px above).
    When it fits on neither side it takes the side with more room and is only as high as that room; the
    rows that do not fit scroll with the selection and with the mouse wheel. The room is the editor area
    less 8 px on every side; the list is moved left as far as needed to stay in it and never reaches out
    of it. A list that follows another one opens at the same place. With the caret's line scrolled out
    of view the nearest shown line counts; before the editor was drawn the list opens at the top of the
    editor as before v5. [OBS-0027 L10, GEN]
14. **Type to filter (v5):** in the lists of behaviour 13 typed characters narrow the rows: shown are the
    rows whose label contains the typed text, upper and lower case alike, in their order; a group heading
    stays while one of its rows is shown. The selection goes to the first row whose label starts with the
    text, else to the first row shown. The text stands in the list's heading line; with no row left the
    list says so and Enter does nothing. Backspace takes the last character away. Escape first clears the
    text and closes the list only when there is none. ↑/↓ move through the shown rows. Enter takes the
    selected row. In a list with check boxes Space toggles the selected row and is never part of the
    text, ⌘A checks the shown rows, and Enter takes every checked row — also checked rows the text
    hides, whose number the heading line then names — or the selected row when none is checked.
    [OBS-0027 L36, GEN]
15. **Override and Implement Members in Kotlin (v6):** `OverrideMethods` (⌃O) and `ImplementMethods`
    (⌃I) work in a class, interface or object of a `.kt` file, with the chooser of behaviours 6, 13 and 14 under
    the titles *Override Members* and *Implement Members*. kotlin-language-server offers this only
    as a quick fix on its error, without a choice, so the members come from the project's syntax
    trees as in SPEC-0082 behaviour 17, without a language server. [OBS-0035 item 8, GEN]
    - *Listed:* the functions and properties of the project supertypes, nearest first, that can be
      overridden (members of an interface; `open`, `abstract` or not final `override` members of a
      class; never `private` ones or extensions) and that the type does not declare itself (also
      not as a `val`/`var` of its primary constructor); then, except in an interface, `equals`,
      `hashCode` and `toString` of `kotlin.Any`. A member declared by several supertypes is listed
      once, under the nearest. Rows read `sound(): String` or `owner: String`. Type parameters of a
      generic supertype are replaced by the arguments the type gives it, also through generic types
      in between (`next(): T` of `Source<T>` reads `next(): String` in a `Source<String>`); a
      member's own type parameters stay.
    - *To implement:* the abstract ones no supertype implements. Implement lists only these, all
      checked; Override lists everything and checks the first.
    - *Added:* at the end of the type's body, after a blank line, indented four spaces deeper than
      the type; a type without a body gets one. A function becomes `override fun name(params):
      Type { … }` with `suspend`, type parameters and `vararg` kept and default values left out; a
      property `override val name: Type` (or `var`) with `get() = …` on the next line and, for a
      `var`, a setter. Where nothing above implements the member the body is
      `TODO("Not yet implemented")` (the setter is empty), else it calls the supertype's
      (`return super.name(args)`, `super.name`). One undo step.
    - *MCP:* `override_methods` takes `.kt` files; `add` matches the name or the row. [ADR-0008]
16. **The name of an extracted constant or variable (v8, OBS-0027 J1):** jdtls names a constant
    after its value (`_25` for `25`) and a variable often with one letter. In a Java file Ferris IDEA
    gives what the server's Extract Constant or Extract Variable (SPEC-0095 behaviour 2)
    introduced a name from the place of the selected expression, in the planned edits themselves:
    the declaration and every use carry it, the name popup that opens right after (SPEC-0095
    behaviour 8) starts from it, and `apply_code_action` reports it as `new_name`. [OBS-0027 J1,
    GEN, STD]
    - *The place,* read from the syntax tree of the text before the edit; the selection must be
      one whole expression, parentheses around it aside. In this order: (a) an argument of a
      method call or of a constructor call (`new`, `this(…)`, `super(…)`): the name of the
      parameter it is passed to, asked from the server with signature help at the start of the
      selection (the active parameter of the active signature; the last word of its label, so
      `String... args` gives `args`); names a class without parameter names gives (`arg0`) do not
      count; (b) the whole initial value of a local variable or a field: that variable's name;
      (c) the value of a `return`: the name of the method around it, without a leading `get`
      (`getLimit` gives `limit`); a `return` in a lambda has none. Any other place keeps the
      server's name.
    - *Extract Constant:* the name in upper case with underscores between its words
      (`unitPriceCents` and `URLParser` give `UNIT_PRICE_CENTS` and `URL_PARSER`). A name the file
      already has anywhere gets `_2`, `_3`, … until none equals it, so it is unique in the class.
      The line stays as the server writes it otherwise, `private static final`.
    - *Extract Variable:* only in place of a server name of one letter or starting with `_`, from
      (a) or from (c) with a method that has the `get` (so `return qty * unitPriceCents;` in
      `totalCents()` keeps `i`, as the reference has it), written as the source name is; never
      when the method around the selection already has that name anywhere (then the server's name
      stays, since a numbered name says no more than a letter).
    - Nothing changes when the name the server introduced was in the file before (it then cannot
      be told from other uses in the edit), when the server does not answer the signature help
      within 5 seconds (v9; the question is then withdrawn with `$/cancelRequest`), or in a Kotlin
      file (kotlin-language-server offers no such extraction).
17. **Inline Method with a choice (v8, OBS-0027 J2):** jdtls offers one action `Inline Method`
    without options; what it does depends on the caret: on the name of a method's declaration it
    inlines every usage and removes the method, on a call it inlines that call and keeps the
    method. For `Inline` (⌥⌘N) and `code_actions` with `inline` in a Java file, Ferris IDEA lists in
    place of that one action the variants it can do with it, in this order, the first marked as
    preferred: [OBS-0027 J2, STD]
    - `Inline Method: all usages, remove the method`: the server's action asked at the
      declaration. From a call the declaration is found with `textDocument/definition`; offered
      there only when the server offers its action at the declaration.
    - `Inline Method: all usages, keep the method`: the server's action asked at every usage
      (`textDocument/references` without the declaration) and their edits put together. Offered
      when the method has at least one usage and all are in this file. Planning refuses, changing
      nothing, when the server offers no Inline Method at one of them (possibly a method reference
      `Item::total`), naming the line, and when two edits touch the same text (a call inside
      another call's argument), saying to inline them one by one.
    - `Inline Method: this usage only, keep the method`: the server's action at the caret; only
      with the caret on a call. Asked with the caret on the declaration it is refused ("put the
      caret on a call of the method").

    With two or three variants the shortcut opens the list (SPEC-0095 behaviour 3) instead of
    applying at once, the first row selected. With fewer than two (the method is declared in
    another file, or the server does not say where) the server's own action stays as it is and is
    applied at once, as before v8: on a call that is this usage only. `apply_code_action` takes
    the variants' titles; the server's own title `Inline Method` still works there and in ⌥↩ and
    ⌃T, which list what the server sends. The titles are Ferris IDEA's own wording. [ADR-0008]

    *A limit (v9):* finding the variants asks the server up to three more questions than its one
    action needed (the declaration, the usages, the action at the declaration); a search for
    usages can take long or, in a server that is busy, not come back. All of them together have
    15 seconds, and never more than the wait the caller gives. When the time is over the questions
    still open are withdrawn (`$/cancelRequest`) and the server's own action stays as it is, as
    with fewer than two variants: Inline applies it at once, which is what it did before v8.
    Planning a variant gives the server the caller's wait for the declaration and the usages and
    the same again for each place's action, and refuses, changing nothing, when the server does
    not say in time where the method is declared and used. For the window, SPEC-0095 behaviour 11
    bounds the whole. [STD, GEN]

## Acceptance criteria
- **AK-1** Markers for Java and Kotlin main and tests, nested classes left out, test method at a
  line; imports of Java (layout, `java.lang`, own package, duplicates, Javadoc use, wildcard,
  static) and Kotlin (conventions, aliases), idempotence, all removed, comments refused;
  constructor, accessors, existing methods, `equals`/`hashCode`, `toString`, imports added
  (`jvm::tests`); single test method configurations (`crates/core/tests/it/run_context.rs`).
- **AK-2** With Maven and Gradle installed: one test method runs alone and the tree has one test
  (`crates/core/tests/it/java_build.rs`).
- **AK-3** MCP: markers with the run command, import preview and write, generate and its refusal
  (`crates/mcp/tests/it/protocol.rs`: `java_editing_tools`).
- **AK-4** Window self-test: markers on class and `main`, the Run/Debug menu, ⌃⌥O, ⌘N → Getter
  and Setter, one ⌘Z undoes it, Run from the marker shows the program's output.
- **AK-6 (v2)** Generate with the caret in a field, on a blank line between members and inside
  a method (unit test `jvm::tests::generated_methods_go_to_the_caret`).
- **AK-5** Hand test (round 4): a Maven/Gradle project next to the reference.
- **AK-7 (v3)** Generate lists what applies for a class with fields, one with an initialised final
  field and a method body (`jvm::tests::generate_offers_what_applies`); Surround With with a hoisted
  declaration, two statements in try/catch and Runnable, refusals (`java_structure::tests`); chooser
  rows grouped and preselected (`workbench_java::tests`); Lombok's jar from the caches
  (`servers::tests::lombok_agent_from_the_local_caches`).
- **AK-8 (v3)** With jdtls: the methods Cat can override (describe of Animal, Object's methods,
  nothing to implement) and adding describe() with `@Override` and `super.describe()`
  (`crates/languages/tests/it/java_structure.rs`).
- **AK-9 (v3)** Window self-test `java_structure_steps`: ⌃I lists `Animal › sound()` checked, Enter
  adds it, one ⌘Z removes it; ⌥⌘T offers the eleven wrappers and `if` keeps the used declaration
  outside with the caret in the condition, one ⌘Z restores the line.
- **AK-10 (v3)** MCP `surround_with` preview and refusal of an unknown kind (`crates/mcp/tests/it/protocol.rs`:
  `java_editing_tools`).

- **AK-11 (v4)** Chooser rows and default checks (`jvm::tests::chooser_lists_and_checks_the_fields`,
  `workbench_java::tests::field_and_template_rows`), chosen fields and refusal of an unknown one
  (`jvm::tests::generate_uses_only_the_chosen_fields`), every template and a template the kind does not
  have (`jvm::tests::generate_templates`); hint text and candidates (`java_generate::tests`).
- **AK-12 (v4)** With jdtls: a field's methods without `java.lang.Object`'s, `start(int)` added as a
  forwarding method; the import candidates of an unresolved `LinkedList`
  (`crates/languages/tests/it/java_generate.rs`).
- **AK-13 (v4)** MCP `generate_choices`, `generate_code` with `fields` and `template` and a refused template
  (`crates/mcp/tests/it/protocol.rs`: `java_editing_tools`).
- **AK-14 (v4)** In a window (open, window check next to the reference): ⌘N with the field and template lists;
  the import hint while typing `LinkedList` and its Escape; Delegate Methods from the Code menu.
- **AK-15 (v5)** Where a list goes: under the caret's line, over it when only that fits, the roomier side
  with a limited height when neither fits, moved left at the right edge, and how many rows fit
  (`popup_place::tests`).
- **AK-16 (v5)** The rows a text leaves, with their headings, and the row selected first
  (`choice_view::tests`).
- **AK-17 (v5)** In a window (`crates/workbench/tests/it/java_generate_lists.rs`): ⌘N opens the list at the
  caret, typed text narrows it, Backspace widens it, Escape clears the text and then closes, Enter takes
  the row left and the field list opens at the same place
  (`the_generate_list_opens_at_the_caret_and_filters`); a list near the bottom of the editor stands over
  the line and a long one is limited to the room and scrolls (`a_long_list_keeps_to_the_room`); check
  boxes with a text (`checked_rows_count_also_when_hidden`); the hint outside a class is gone after a
  caret move and when a list opens (`the_hint_outside_a_class_passes`). With jdtls
  (`crates/workbench/tests/java_delegate_import.rs`): typing narrows the method list of Delegate Methods
  and Enter adds the method left; Show Context Actions with one class in the hint adds the import
  (`typing_narrows_the_delegate_list_and_the_hint_imports`).
- **AK-18 (v5)** In a window next to the reference (open): the places of the lists and the import from the hint.
- **AK-19 (v6)** For `Dog : Animal(name), Pet` of OBS-0035 the list is `describe(): String` of
  `zoo.Animal`, `owner: String` (to implement) and `friendly(): Boolean` of `zoo.Pet`, then the three
  of `kotlin.Any`; adding `owner` and `describe` appends the property with `TODO` and the function
  calling `super`; `Cat`, which declares `owner` in its constructor, has nothing to implement; a
  class without a body gets one, an empty body is filled, `var`, `suspend`, type parameters,
  `vararg` and functions without a return type keep their shape, type arguments replace the
  supertype's type parameters also through a generic class in between, and what an object declares
  is not offered again (unit tests `kotlin_structure::tests::override_and_implement_members`,
  `stubs_and_bodies`; chooser rows `workbench_java::tests::override_rows_show_kotlin_labels`; on
  files on disk `crates/languages/tests/it/kotlin_structure.rs`).
- **AK-20 (v6)** In a window (`crates/workbench/tests/it/kotlin_structure.rs`): `ImplementMethods`
  (bound to ⌃I) on `Dog` opens *Implement Members* with `zoo.Pet` and `owner: String` checked, Enter
  adds the property, a second ⌃I says there is nothing to implement; `OverrideMethods` (⌃O) on a
  class without body lists `kotlin.Any`'s three and adds the chosen one in a new body. MCP
  `override_methods` lists and adds for a `.kt` file (`crates/mcp/tests/it/protocol.rs`).

- **AK-21 (v7)** `java_structure::tests::{return_types_come_from_the_project_sources,
  default_methods_of_java_interfaces}` (the open text, another file by package and name, a nested
  type, build output and a library type without one, the label);
  `workbench_java::tests::implement_lists_default_methods_unchecked`. With a real jdtls and in the
  window: to be checked on the Mac (*Select Methods to Implement* on `Dog` lists `owner(): String`
  checked and `friendly()`'s default unchecked, OBS-0027 project).

- **AK-22 (v8, behaviour 16)** The place of a selected expression (argument, initial value,
  returned value, also in parentheses; none for a part of an expression, two arguments, half a
  literal) and the names (`UNIT_PRICE_CENTS`, a taken name numbered, `arg3` and an unknown
  parameter keep the server's name, a variable only for a poor server name, without a clash and
  not from a method without `get`): `java_refactor::tests::{the_place_of_an_expression_names_it,
  names_follow_the_place}`. With jdtls on the file of OBS-0027
  (`crates/languages/tests/it/java_refactor.rs`, skipped without it): Extract Constant on `25` in
  `new Item("A-1", "Bolt", 3, 25)` plans `private static final int UNIT_PRICE_CENTS = 25;` and the
  use `3, UNIT_PRICE_CENTS)`, no `_25` is left and the introduced name found is that one; Extract
  Variable there plans `int unitPriceCents = 25;`; on `qty * unitPriceCents` in `totalCents()` it
  keeps `int i`. MCP (`protocol.rs::java_constant_name_and_inline_method_variants`):
  `apply_code_action` reports `new_name` `UNIT_PRICE_CENTS`.
- **AK-23 (v8, behaviour 17)** A declaration's name is told from a call, the variants count as
  Inline and introduce no name, overlapping edits are refused with their line
  (`java_refactor::tests::inline_knows_a_declaration_from_a_call`). With jdtls (same live test):
  on the declaration of `totalValue` the two "all usages" variants, on a call all three, in the
  order of behaviour 17; "remove" leaves no `totalValue` and inlines the three calls; "keep" inlines
  the three calls and keeps the declaration; "this usage" changes the one call; from the call
  and from the declaration "remove" and "keep" plan the same text; "this usage" on the declaration
  is refused; the server's own title plans what it did before. MCP (same protocol test): the
  titles at a call and at the declaration, and "this usage only" applied to the file. In a window
  with the server pool, ⌥⌘N on the declaration's name opens the list with the two "all usages"
  variants (`crates/workbench/tests/java_delegate_import.rs::inline_on_a_method_lists_its_variants`,
  skipped without jdtls); on the screen: to be checked on the Mac.
- **AK-24 (v9, behaviours 16 and 17)** Against a scripted server that answers nothing, with a
  limit per request of ten minutes as a server from the pool may have: the variants are given up
  after their time, on the declaration (the usages are asked) and on a call (the declaration is
  asked), the server's own action is what is listed, and the open question is withdrawn with
  `$/cancelRequest` (`java_refactor::tests::the_variants_give_way_when_the_server_is_silent`).
- **AK-25 (v10, behaviour 3e)** Unit tests (`test_class::tests::{a_new_test_goes_into_the_test_root,
  an_existing_test_is_found_and_tests_are_refused}`): the test folder for Maven, Gradle modules,
  a top-level `src/` and a flat file, the package line, `public` for JUnit 4 alone; an existing
  test by name or on disk is opened; a test file and another language are refused. MCP:
  `create_test` (`crates/mcp/tests/it/protocol.rs::create_test_plans_and_writes_a_new_class`).
- **AK-26 (v10, behaviours 3d and 3e)** In a window of the test harness, by keys
  (`crates/workbench/tests/it/keymap_gaps_obs0119.rs::generate_lists_the_choosers_test_and_copyright`):
  ⌘N in a class with fields that implements an interface, with a copyright profile, lists the
  kinds, Override Methods…, Implement Methods…, Delegate Methods…, Test… and Copyright; `test`
  Enter creates `src/test/java/shop/PersonTest.java`, opens it with the caret in the body; the
  second time it opens the same file unchanged; Copyright writes the notice. A class without
  supertype and without profile lists no Implement Methods… and no Copyright
  (`java_generate_lists.rs`).

## Out of scope
Generate's choosers for Kotlin (SPEC-0132), the reference's custom template editor, Javadoc options of Generate,
Surround With and Delegate Methods for Kotlin, records and enums beyond their fields,
Javadoc, debugging tests, adding imports by itself while typing (behaviour 11 only announces), a setting
to turn the import hint off, the place of the Surround With list and of the run marker's menu,
marking the matched text in a filtered row.

## Known deviations from the reference
Imports are never collapsed into `*` (the reference does so from five imports of one package),
because a wildcard hides what is used and can make names ambiguous when a package gains a class
(IMP-0045). Everything works from the syntax tree, so it also works while the language server is
still importing the project. Compared side by side in OBS-0027 (v3): Implement Methods listed only
the methods still to implement (the reference also lists default methods, unchecked) and showed no
return types (jdtls does not send them); both since v7 (behaviours 6a, 6b), return types only for
types with a source in the project; Lombok works only with the jar in the local caches and
never in safe mode, because jdtls would run code of the project's dependency (IMP-0381).
Generate asks two short lists in a row where the reference uses one dialog with options (v4); the
templates are Ferris IDEA's own fixed set, not the reference's editable ones. Delegate Methods leaves out
what every object has (v4). Not compared next to the reference yet. The lists filter their rows by a
typed text and Show Context Actions imports from a shown hint at once (v5); neither was compared next to
the reference (OBS-0027 L36, L37).

v6 (OBS-0035): the Kotlin choosers know the supertypes declared in the project's `.kt` files; members
of library types other than `kotlin.Any`'s three (`Comparable.compareTo`, Java supertypes) are not
listed, where the reference lists them from its index. A member two supertypes both
implement is added with a plain `super.` call, which Kotlin then asks to qualify. There is no option to
copy the documentation comment.

v8 (OBS-0027 J1, J2): the extracted constant is `private` as jdtls writes it, where the reference
writes `public`; the narrower visibility is kept on purpose (nothing outside the class uses a
constant that was a literal a moment ago; widening is one edit). The reference proposes several
names in a list and takes them from more places (the type, the expression's own words); Ferris IDEA
has one name from three places and else the server's. Inline Method: the reference asks in a
dialog with a preview and can also inline usages in other files while keeping the method; Ferris IDEA
lists the variants as rows of the refactoring list, offers "all usages, keep the method" only
while every usage is in the file, and offers no choice at all for a method declared in another
file (then jdtls's single action: this usage at a call), because jdtls 1.64.0 has no option for it
— its `Inline Method` carries no arguments, and the same request gave "all usages, method removed"
at the declaration and "this call" at a call. Inline of a variable or a constant is the server's
single action as before. Not compared in a window yet.
