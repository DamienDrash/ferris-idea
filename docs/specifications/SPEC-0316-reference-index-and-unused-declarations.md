# SPEC-0316: Project reference index and unused declarations

- **Version:** 4 · **Date:** 2026-11-15 (v4: Kotlin declarations in the index, uses across Java
  and Kotlin both ways, Kotlin entry points, `kotlin.unused-declaration`, behaviours 9 and 10, AK-10;
  decision of the coordinator under the Project Owner's general authority of 2026-11-13); 2026-11-14 (v3: the findings also in the editor, behaviours 5 and 8;
  more entry points and the API folders of a library, behaviour 4; AK-8, AK-9); 2026-11-13 (v2:
  which files the index reads, behaviour 7)
- **Sources:** OBS-0019 item 1 and its open questions (the reference reports public methods and
  classes without callers as unused, from a project-wide analysis; defect F032-4), GEN (an unused
  declaration check needs the callers of every member across the project, so IDEs keep an index
  of names and their uses and run the check as a global inspection of Inspect Code, not while
  typing; entry points such as `main`, test methods and framework-managed members count as used),
  STD (Java Language Specification: declarations, `@Override`, `main(String[])`, the methods of
  `java.lang.Object`, the serialization methods), OSS (`JetBrains/intellij-community`: the short
  name `unused` of the unused declaration inspection, used for suppression), SPEC-0098 v4,
  SPEC-0125 v3, SPEC-0315 v2, ADR-0008; v3: OBS-0050 (the reference marks unused non-private
  members and a nested class in the editor, not the public top-level class), SPEC-0098 v12, STD
  (Java Language Specification: `native` methods, members of interfaces are implicitly public;
  JEP 512, Java 25: a `main` method may be an instance method and may have no parameters; Java
  Object Serialization Specification: `serialPersistentFields`), GEN (test runners and frameworks
  that are named on a class, such as TestNG's `@Test` or a REST resource's `@Path`, call its
  public methods; a library's public API is used by code outside the project); v4: STD (Kotlin
  language specification: declarations, default visibility `public`, `internal`, `override` is
  mandatory, `operator` functions called through operators and conventions — `get`, `invoke`,
  `contains`, `iterator`, `componentN`, `getValue`/`setValue`/`provideDelegate` of delegated
  properties; `infix` calls name the function; `data` classes derive `componentN`, `copy`,
  `equals`, `hashCode`, `toString` from the primary constructor's properties;
  `expect`/`actual`, `external`, `const val`, enum entries, companion objects, object
  expressions, string templates `$name`; Kotlin/JVM interoperability: a property is seen from Java
  as `getName`/`setName`, a property `isOn` as `isOn`/`setOn`, a top-level function as a static
  method of the file class `FileKt`, a companion member through `Companion`, `@JvmField`,
  `@JvmStatic` and `const val` as fields or static members; Java getters and setters are seen from
  Kotlin as properties), GEN (Ktor loads a module, an extension function of `Application`, by the
  name written in `application.conf` or `application.yaml`; kotlinx.serialization and Spring read
  the properties and call the functions of an annotated class; test runners call annotated
  functions)
- **Feature matrix rows:** F-032

## Purpose
Find classes, methods and fields that nothing in the project uses, without asking a language
server about every member, and keep the knowledge needed for that cheap to update.

## Behaviour
1. **Index:** Ferris IDEA keeps a project-wide reference index. For each Java file it holds the
   declarations of types (class, interface, enum, record, annotation type), methods and fields with
   their name, kind, enclosing type, position, the facts of behaviour 4, and the supertypes named by
   each type; and the number of uses of every name in the file: each identifier and type name that
   is not the name of a declaration of behaviour 1 and not inside an `import` or `package`
   declaration. For the other text files that can name Java classes (`.kt`, `.kts`, `.groovy`,
   `.gradle`, `.xml`, `.properties`, `.yml`, `.yaml` and files below `META-INF/services/`; v4:
   also `.conf`, and `.kt` files are indexed by behaviour 9 instead) it holds
   the number of uses of each word. Files over 1 MiB, unreadable and non-UTF-8 files are left out.
   The index is built from Ferris IDEA's own Java syntax trees, because asking the language server for
   the references of every member costs one request per member and needs a running server. [GEN]
2. **Incremental and cached:** each file's entry carries the file's modification time and size. A
   refresh lists the project files, reads and parses only files that are new or whose time or size
   changed, and drops entries of files that are gone. The index lives in memory for the process and
   is saved to Ferris IDEA's user cache folder (`reference-index/<hash of the project path>.json`), so a
   new process (a window or a one-shot `ferris-idea call`) starts from the saved state; a saved index
   of another format version is ignored. Saving removes cached indexes whose project folder no
   longer exists. Every refresh reports how many files are indexed, how many were read and removed,
   and whether the saved index was used. [GEN]
3. **Unused declarations:** a declaration of behaviour 1 that is not private and not an entry point
   (behaviour 4) is unused when its name has no use in any indexed file. Uses are counted by simple
   name, so a name used anywhere keeps every declaration of that name; a use inside the declaration
   itself (a recursive call, a field of its own type) counts as a use. Private members stay with the
   file's own rule `java.unused-private` (SPEC-0125). [GEN]
4. **Entry points and members that are used from outside the source (never reported):** [GEN, STD]
   - any declaration with an annotation (`@Override`, `@Test`, framework annotations, also
     `@SuppressWarnings`);
   - `static void main(String[] …)` and every type that declares one, or that declares an annotated
     method or field (test classes, framework components);
   - constructors, enum constants, record components, members of annotation types, members of
     anonymous classes and of enum constant bodies;
   - the methods `toString`, `equals`, `hashCode`, `clone`, `finalize`, the serialization methods
     (`readObject`, `writeObject`, `readResolve`, `writeReplace`, `readObjectNoData`) and the field
     `serialVersionUID`;
   - methods of a type one of whose supertypes, followed through the project's types, is not
     declared in the project, because they may implement a library method.
   - v3, in addition:
     - a method named `main` whose parameters are none or one `String[]`/`String...`, static or
       not, of any visibility (the launch protocol of Java 25 accepts all of them), and its type;
     - `native` methods (called from native code);
     - the field `serialPersistentFields`;
     - the public methods of a type that carries an annotation other than `@SuppressWarnings`,
       `@Deprecated` and `@FunctionalInterface` of `java.lang` (TestNG's class-level `@Test`, a
       JAX-RS `@Path`, a Spring `@RestController`: the framework calls them; before v3 only the
       type itself was kept);
     - **API folders of a library (configured):** the file `.ferris-idea/entry-points.toml` of the
       project may list folders, relative to the project, whose code is a library used from outside
       the project: `api = ["lib/src/main/java"]`. In files below them every `public` or
       `protected` declaration and every member of an interface or annotation type is an entry
       point. Without the file, or with an unreadable one, there are no API folders; a folder that
       is absolute, contains `..` or lies behind a symbolic link is ignored (security first). The
       file is read at every run, so a change applies at the next one without a refresh.
5. **Global inspection:** the finding is Ferris IDEA inspection `java.unused-declaration` ("Unused
   declaration", warning by default, short name `unused`). It is a global inspection: it runs in
   Inspect Code (SPEC-0098) and for agents, never in the editor while typing. Inspect Code refreshes
   the index over the whole project, whatever the scope, and reports the unused declarations of the
   files in the scope, on the declaration's name, with the message "Class 'Name' is never used"
   (Interface, Enum, Record, Annotation type), "Method 'name()' is never used" or "Field 'name' is
   never used". It follows the project's profile (SPEC-0315) and the suppressions of SPEC-0125 v2
   behaviour 7 (`unused`, `ALL`, the id). It has no quick fix. [OBS-0019, OSS] (v3: it is also
   shown in the editor, behaviour 8; there it has the quick fix Safe Delete.)
6. **MCP:** `inspect_code` includes the global inspection (SPEC-0098 v4). `reference_index`
   refreshes the index and returns its statistics (files, Java files, read, removed, `cached`); with
   `name` the declarations of that name (path, 0-based line and column, kind, enclosing type) and
   the files that use it with their counts; with `unused=true` the unused declarations, limited to a
   file or folder with `path`, as `inspect_code` places them. [ADR-0008]
7. **Which files are read (v2; decision of the coordinator of 2026-11-13 under the Project Owner's general authority: the rule the Project Owner chose for Rename, Find in Files, Search Everywhere, Inspect Code and Endpoints, applied to a reader):** before v2 the index was built
   over every file of the workspace except a short list of folder names, so a use of a name in
   `.venv` or in a copy below `build` kept a declaration "used". Since v2 the index reads
   - the project's own sources by the rule of SPEC-0148 v4 behaviour 7 (`OwnSources`), and
   - the files that are left out of them **only** because an ignore rule of Git matches: a
     generated source of the project (`src/gen/Parser.java` named in `.gitignore`) is not
     committed, but it is compiled with the project and its uses are real. Leaving it out would
     report declarations as unused that are used, and an unused finding invites deleting them.
   It does not read, also not inside an ignored folder: virtual environments, installed
   dependencies, build output and caches (`target`, and `build`, `out`, `dist` outside `src`),
   version control and IDE data, folders the project model excludes, and anything behind a
   symbolic link. A use there is not a use by the project; a stale copy in build output would
   hide a declaration that became unused. Known limit: sources generated into build output
   (`build/generated`, `target/generated-sources`) do not count as uses; `target` was never
   read, `build` no longer is. There is no option.
   **Scope of the findings:** unused declarations are reported for the files Inspect Code runs
   over (SPEC-0098 v8 behaviour 16): the project's own files, or a place named on purpose — not
   for a merely ignored file read for its uses, which is generated and not the project's to
   tidy up. `reference_index` with `unused` chooses its files the same way. [GEN]

8. **In the editor (v3, SPEC-0098 v12 behaviour 26):** the editor of a Java file asks the index,
   once it is ready (built at least once in this process for the project), for the unused
   declarations of that file with the file's current text in place of its indexed entry: its
   declarations, uses and types from the editor's syntax tree, every other file's uses and types
   from the index. Totals of the uses per name and the project's types are kept with the index and
   computed again after each refresh that changed it, so the step costs the walk of the one tree
   and a lookup per declaration, not a pass over the project. Top-level types are not reported
   there. The step checks a cancel flag between its parts and returns nothing once it is set, and
   nothing while a refresh of that project holds its index (the refresh's end asks again; each
   project has its own lock, so a refresh of another project does not count). Saved format 2 (the
   declarations carry their visibility for behaviour 4); a saved index of format 1 is read anew.
   [GEN]

9. **Kotlin declarations and uses across the languages (v4):** for each Kotlin file (`.kt`; Kotlin
   scripts `.kts` stay word files of behaviour 1) the index holds, from Ferris IDEA's Kotlin syntax
   tree, the declarations of classes (also `data`, `sealed`, `value`, `enum` and `annotation`
   classes), interfaces, named objects, functions (members, top-level, extension functions, in
   objects and companion objects) and properties (members, top-level, extension properties, and
   the `val`/`var` parameters of a primary constructor) with name, kind, enclosing type (the
   companion object's name, `Companion` when it has none), position, visibility (no modifier is
   public) and the facts of behaviour 10; the supertypes of every class, interface, object and
   companion object; and the uses: every identifier that is not the name of such a declaration or
   of an enum entry and not inside the `package` header, an `import` or a `@file:` annotation, and
   every `$name` in a string template. Declarations inside function bodies, lambdas,
   initialisers, accessors and secondary constructors are local and not indexed; their names
   count as uses. Comments do not count (before v4 every word of a `.kt` file counted).
   Kotlin types take part in the supertype rule of behaviour 4 like Java types. Uses count by
   simple name across the two languages, both ways:
   - a Java use of a Kotlin declaration by its name counts (`Util.helper()` for a top-level
     function through `UtilKt`, `Config.LIMIT` for a `const val` or `@JvmField` property,
     `Box.Companion.make()`); a Kotlin **property** `name` is also used by every use of the names
     of its Java accessors, `getName` and `setName` (for a property named `isOn`: `isOn` and
     `setOn`);
   - a Kotlin use of a Java declaration by its name counts; a name `name` that a Kotlin file
     reads or writes as a property (not called) also counts for `getName` and `setName`, and
     `isOn` for `setOn`, because Kotlin reads Java getters and setters as properties.
   The finding of a Kotlin declaration is Ferris IDEA inspection `kotlin.unused-declaration`
   ("Unused declaration", Kotlin, warning by default, short name `unused`), with the messages
   "Class 'Name' is never used" (Interface, Enum class, Annotation class, Object), "Function
   'name' is never used" and "Property 'name' is never used". Inspect Code reports it as behaviour
   5 reports Java's, top-level types included, and follows the profile for each of the two ids on
   its own; `reference_index` lists Kotlin declarations with the kinds `function`, `property`,
   `object`, `enum-class` and `annotation-class` besides the Java kinds, and its statistics
   count `kotlin_files`. Private Kotlin declarations stay with `kotlin.unused-private`
   (SPEC-0140). Suppression: SPEC-0098 v13 behaviour 28. Saved format 3 (Kotlin files hold
   declarations); a saved index of format 2 is read anew. [STD, GEN]
10. **Kotlin entry points and declarations used from outside the source (v4; never reported):**
    [STD, GEN]
    - any declaration with an annotation (`@Test`, `@JvmStatic`, `@JvmField`, `@Serializable`,
      Spring's `@Bean` or `@GetMapping`, `@Composable`, also `@Suppress`), and every class or object
      that declares an annotated function or property (a test class, a Spring configuration);
    - the public functions and properties of a class or object that carries an annotation other
      than `@Suppress`, `@Deprecated`, `@OptIn` and `@JvmInline` (a `@RestController`'s
      handlers, the properties of a `@Serializable` class);
    - a function named `main` with no parameters or one `Array<String>` (or `vararg` `String`)
      parameter;
    - `override` declarations; `operator` functions (called through `+`, `[]`, `in`, `for`,
      `()`, destructuring and `by` without their name); `infix` functions need no rule, their
      calls name them;
    - `expect` and `actual` declarations, `external` functions;
    - the constructor properties of `data` classes (`componentN`, `copy`, `equals`, `toString`),
      of annotation classes (read through the annotation) and of `value` classes; enum entries;
      companion objects themselves (used through their members, which are checked); the members of
      object expressions (`object : Runnable { … }`), of enum entry bodies and of annotation
      classes;
    - the functions `readObject`, `writeObject`, `readResolve`, `writeReplace`, `readObjectNoData`
      and the properties `serialVersionUID` and `serialPersistentFields` (Java serialization);
    - a public extension function whose receiver is `Application` (a Ktor module, loaded by the
      name in `application.conf` or `application.yaml`; `.conf` files are word files of
      behaviour 1 since v4, so a module named there is used anyway); routes are lambdas and
      need no rule, the classes of typed routes carry `@Resource`;
    - in an API folder of behaviour 4: every public (no modifier or `public`) or `protected`
      declaration and every member of an interface or annotation class; `internal` is not API.
    `const val` and `@JvmField` need no further rule: Java uses them by the property's name, which
    counts (behaviour 9), and `@JvmField` is an annotation.

## Acceptance criteria
- **AK-1** Declarations, uses (not counting declaration names, imports and package lines), word
  uses of other text files; an unused public method, class and field are found, a used one, an
  annotated one, `main` and its class, Object methods, a method of a class with a library supertype
  and a member of an anonymous class are not; suppression with `@SuppressWarnings("unused")` on the
  class (unit tests `reference_index::tests`).
- **AK-2** Refresh reads only changed and new files and drops removed ones; a saved index is loaded
  by a new index and used without reading the files again; another format version is ignored;
  saving removes the cache of a project folder that is gone (unit tests `reference_index::tests`).
- **AK-3** `inspect_project` reports `java.unused-declaration` for files in the scope with uses from
  files outside it; turned off in the profile it is not reported (unit test `inspections::tests`).
- **AK-4** MCP: `reference_index` statistics, `name`, `unused` and its `path` scope, a second call
  that reads no file; `inspect_code` lists the unused method (`crates/mcp/tests/it/protocol.rs`:
  `reference_index_tool`); `ferris-idea call <folder> reference_index` on `java-basic`.
- **AK-5** Window self-test (`java_inspection_steps`, written): Inspect Code over the project lists
  the node "Unused declaration" with the unused method of the fixture.
- **AK-6 (v2)** The files read: own sources and merely ignored files, not `.venv`,
  `node_modules`, build output, IDE data, and not `node_modules` inside an ignored folder
  (`own_sources::tests::sources_and_ignored_files_without_environments_and_output`).
- **AK-7 (v2)** MCP in a temporary project: a method used only from an ignored generated file is
  not reported and `name` lists that use; methods used only from
  `.venv/lib/python3.12/site-packages/x.java` or `node_modules/y.java` are reported as unused;
  the generated file's own declarations are not reported
  (`crates/mcp/tests/it/protocol.rs::spring_beans_and_reference_index_read_the_projects_own_sources`).

- **AK-8 (v3, behaviour 4)** An instance `main()` without parameters and its class, a `native`
  method, `serialPersistentFields`, a public method of an annotated class are not reported; a
  public method and a protected field below a configured API folder are not reported, the same
  declarations elsewhere and a package-private one there are; an absolute or `..` folder in
  `.ferris-idea/entry-points.toml` is ignored (unit tests `reference_index::tests::{more_entry_points,
  api_folders_of_a_library}`).
- **AK-9 (v3, behaviour 8)** As SPEC-0098 AK-16 (unit tests `reference_index::tests::{editor_findings_of_the_unsaved_text,
  editor_findings_wait_for_the_index_and_stop_when_cancelled}`, window test
  `java_unused_window.rs`).

- **AK-10 (v4, behaviours 9 and 10)** As SPEC-0098 AK-17; besides, unit tests
  `reference_index::tests::{kotlin_declarations_and_entry_points, uses_across_java_and_kotlin}`
  (declarations and kinds of a Kotlin file, the entry points of behaviour 10 not reported, a
  Kotlin function used only from Java, a property used only through its Java getter and a Java
  getter used only as a Kotlin property not reported, an unused Kotlin function, property, object
  and class reported, saved format 3) and `crates/mcp/tests/it/protocol.rs::reference_index_tool`
  (a Kotlin declaration in `name` and `unused`).

## Out of scope
Declarations of languages other than Java and Kotlin (v4: Kotlin since v4; Kotlin scripts, `@JvmName`
names seen from Java, multiplatform source sets as separate modules), uses through
`::class` reflection and `kotlin-reflect`, resolving uses by type (overloads, same names in other
classes), the reference's options of the inspection (which members and visibilities to check,
entry-point annotations to configure), "Safe delete" as a fix in the Inspect Code results (v3: in
the editor it is one), uses from reflection strings.

## Known deviations from the reference
The reference resolves every use to its declaration; Ferris IDEA counts uses by simple name and
therefore keeps a declaration whose name is used for something else (fewer findings, never a wrong
one for a name that has no use at all). A recursive call counts as a use. Entry points are decided
by annotation and supertype instead of the reference's framework knowledge. Improvement: the index
is kept on disk and refreshed incrementally, so a rerun reads only changed files, and agents can ask
the index which files use a name, which explains every verdict (IMP-0309).
Since v2 uses in environments, dependencies and build output do not count (behaviour 7); the
reference was not compared for this.
v4: the reference's Kotlin plugin was not observed for this; the Kotlin entry points follow the
language and the frameworks named in the sources. Uses across the languages count by name and by
the JVM accessor names, so a Java `getName()` keeps every Kotlin property `name` (fewer findings,
never one for a property whose name and accessors have no use at all).
