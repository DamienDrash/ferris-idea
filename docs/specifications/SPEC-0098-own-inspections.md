# SPEC-0098: Own inspections and Inspect Code

- **Version:** 13 · **Date:** 2026-11-15 (v13: unused declarations of Kotlin files in the editor, `@Suppress` for IDECraft's Kotlin findings, behaviours 27 and 28, AK-17; decision of the coordinator under the Project Owner's general authority of 2026-11-13, no observation of the reference); 2026-11-14 (v12: unused declarations in the editor from the project reference index, behaviour 26, AK-16, OBS-0050 W1, W2, W6, W7; v11: Java lambda simplifications, behaviours 22–25, AK-14 and AK-15, OBS-0050; v10: Inspect Current File and Run Inspection by Name from the keymap, behaviours 20 and 21, AK-13; eighth package of docs/research/KEYMAP_COVERAGE.md; v9: IDECraft's own Python quick fixes at an unused import, an unused local and a name not defined, behaviours 17–19, AK-12, OBS-0034 part 14 Q1–Q3; decisions of the agent while the Project Owner was not reachable); 2026-11-12 (v3: descriptions, scope dialog, results by inspection with a
  description pane and fixes, the project's profile — OBS-0019 items 3, 6, 7; v4: one inspection on a
  scope, global inspections — defects F032-2, F032-4; v5: Python names that shadow a name of an outer
  scope, own findings in `file_diagnostics` — OBS-0034 L6; v6: Python methods that do not use
  `self` — OBS-0034 L12; v7: the quick fix "Make method static" for them; v8: a whole-project run
  inspects the project's own sources — decision of the Project Owner of 2026-11-12)
- **Sources:** GEN (code inspections beyond compiler errors, marked while typing and run for the whole
  project with Code | Inspect Code, results grouped by file; F-032), OSS (action id `InspectCode`),
  SPEC-0070 (problems in the editor), SPEC-0092 (Problems tool window); v3: OBS-0019 items 3, 6, 7, SPEC-0315 (profile); v4: OBS-0019 item 4
  ("apply the inspection to a scope" in the submenu), SPEC-0125 v3, SPEC-0316 (reference index);
  v5: OBS-0034 item 33 (the reference reports a parameter that has the name of a module-level
  variable as shadowing a name of the outer scope, pyright does not), STD (The Python Language
  Reference, "Naming and binding": a name bound in a function is local to it unless declared
  `global` or `nonlocal`; the scope of a class body does not extend to its methods; what binds a
  name), SPEC-0023 (`file_diagnostics`); v6: OBS-0034 item 44 (the reference reports a method
  that does not use `self` as one that could be static, pyright does not), STD (The Python
  Language Reference, "Class definitions" and "Function definitions"; `staticmethod` in the
  library reference; zero-argument `super()` needs the instance), GEN (pytest collects methods
  of `Test…` classes and calls them on an instance); v7: decision of the coordinator 2026-11-12
  (the weak warning gets a fix), STD (The Python Language Reference, "Function definitions":
  decorators, the `/` and `*` markers of a parameter list; library reference, `staticmethod`: a
  static method can be called on the class and on an instance and receives no implicit first
  argument), SPEC-0125 (quick fixes of own inspections, `fix_inspection`); v11: OBS-0050 (seven warnings of the reference in a small Java file: four unused declarations,
  an anonymous class that can be a lambda, a block lambda with one statement, a call that can be a
  method reference; S1, S2), STD (Java Language Specification: lambda expressions, method
  reference expressions, functional interfaces; a lambda cannot declare a name the enclosing
  method already uses, and `this` in a lambda is the enclosing instance), OSS
  (`JetBrains/intellij-community`: the short names `Convert2Lambda`, `CodeBlock2Expr`,
  `Convert2MethodRef` used for suppression); v12: OBS-0050 (the
  reference marks four non-private declarations without uses in the editor of a one-file project,
  but not the file's public top-level class), SPEC-0316 v3, SPEC-0095 v12 behaviour 15 (Safe
  Delete), SPEC-0260 v4, GEN (an analysis that needs the whole project runs after the edit on a
  background thread against an index of the other files and is thrown away when the text changed);
  v13: SPEC-0316 v4, STD (Kotlin language specification: annotations with use-site target
  `@file:`, which stand before the `package` header; `kotlin.Suppress` takes the names to
  suppress as strings), OSS (`JetBrains/intellij-community`, Kotlin plugin: the unused symbol
  inspection is suppressed with the name `unused`, an unused parameter with
  `UNUSED_PARAMETER`), GEN (IDEs offer to suppress a Kotlin warning for the declaration, the
  enclosing class or the whole file)
- **Feature matrix rows:** F-032, F-035

## Purpose
Catch likely bugs that the language servers do not report, in the editor and for the whole project, for
people and agents alike.

## Behaviour
1. **Rules:** IDECraft has its own inspections, each a tree-sitter query with a check, a stable id, a
   name, a level (warning or weak warning) and a message. The first set: [GEN]
   - `java.empty-catch`, `kotlin.empty-catch`: a `catch` block with nothing in it, not even a comment;
   - `java.string-equality`: `==`/`!=` between a string literal and an expression other than `null`;
   - `python.bare-except`: `except:` without an exception type (weak);
   - `js.coercing-equality`: `==`/`!=` in JavaScript or TypeScript, except against `null` or
     `undefined` (weak);
   - `any.conflict-marker`: Git conflict marker lines, in any text file, but only in files that have an
     opening `<<<<<<<` line (so a Markdown `=======` underline is not reported).
   - v2: the Java rules and quick fixes of SPEC-0125.
2. Unsupported languages and unparsable text produce no syntax findings; conflict markers are still
   found.
3. **Editor:** findings appear with the language-server problems (SPEC-0070): wavy underline, counters,
   F2/⇧F2, Problems tool window. Their message starts with "IDECraft: ". They are recomputed whenever
   the syntax tree is reparsed, never on stale trees. [GEN]
4. **Inspect Code:** the action `InspectCode` (Search Everywhere; no default shortcut, as in the
   reference) runs every inspection over the project files off the UI thread (files over 1 MiB,
   unreadable and non-UTF-8 files are skipped) and opens the Problems tool window on its **Inspection**
   tab: findings grouped by file as "Name: message" with their line; Enter or a double click opens the
   place. While running, the tab says "Inspecting code…". [GEN]
5. Agents run the same inspections with `inspect_code` (whole project or a file or folder); each
   finding has path, 0-based line and column, char offset, id, name, level and message (v8: in
   `findings`, behaviour 16). [ADR-0008]

6. **Descriptions (v3):** every inspection has a description in IDECraft's own words: what it reports,
   why it matters, and a short example of the reported code and, where a fix exists, of the fixed
   code. It is shown by a second ⌘F1 (SPEC-0078 v2), in the description pane of the results
   (behaviour 8) and on the settings page (SPEC-0315). Agents read it with `inspection_profile`
   (SPEC-0315). [OBS-0019]
7. **Scope dialog (v3):** `InspectCode` first opens a small dialog: the scope — **Whole project**
   (default), **Current file** (when an editor is active) or **Folder of the current file** — and the
   profile in use ("Project Default", with a link that opens Settings › Editor › Inspections);
   **Analyze** (Enter) runs, **Cancel** (Escape) closes. Up/Down change the scope. [OBS-0019]
8. **Results (v3, replaces the grouping of behaviour 4):** the Inspection tab of the Problems tool window
   is named after the scope ("Inspection: Project", "Inspection: Problems.java"). Its tree has a root
   with the profile and the number of findings, then one node per inspection (name, count; sorted by
   name), under it the files (name, count) and under each file the findings (message, `:line`). Nodes
   fold with ←/→ or a click on the chevron; all are open after a run. A **Rerun** button repeats the
   run with the same scope. The right half of the tab is a description pane: [OBS-0019]
   - an inspection selected: its name, its description, and — when its findings have fixes — a
     button **Fix all** ("Fix all 'Name' problems") that applies every fix of the listed findings;
   - a finding selected: its message, the lines around it from the file, a button with the fix title
     when it has one, and a button **Suppress** (Java: for the statement, SPEC-0125 v2);
   - nothing selected: a hint to select an inspection.
   A fix from the results changes the open editor (one undo step) or, for a file that is not open,
   the file on disk; the fixed findings leave the tree at once. Enter or a double click on a finding
   still opens its place.
9. **Profile (v3):** inspections that are turned off in the project's profile (SPEC-0315) are not run,
   neither in the editor nor in Inspect Code nor for agents; a severity set there replaces the level
   of behaviour 1 (error, warning or weak warning). Suppressed findings (SPEC-0125 v2 behaviour 7)
   are not reported. [OBS-0019]
10. **One inspection on a scope (v4):** "Run inspection on…" in a finding's submenu (SPEC-0125 v3)
    opens the scope dialog of behaviour 7 with the title "Run 'Name' on: scope"; Analyze runs only
    that inspection, even when the profile turns it off, and the results tab is named
    "Inspection: Name". Rerun keeps the limit; a later Inspect Code from the menu runs all
    inspections again. `inspect_code` takes `id` to run one inspection the same way. [OBS-0019]
11. **Global inspections (v4):** an inspection that needs the whole project runs only here and for
    agents, never in the editor: `java.unused-declaration` of SPEC-0316. It appears in the results
    tree like the others; its findings have no fix. [GEN] (v12: `java.unused-declaration` is also
    shown in the editor, behaviour 26; in the results it still has no fix.)
12. **Python: a name that shadows one of an outer scope (v5):** inspection `python.shadows-name`
    ("Name shadows a name of an outer scope", weak warning) reports, in a function (`def`), a name
    that the function binds itself and that an enclosing scope binds too, because inside the
    function the outer name can no longer be read and a later reader may take one for the other.
    [OBS-0034 item 33, STD]
    - **Reported names:** the function's parameters (plain, with default, annotated, `*args`,
      `**kwargs`, keyword-only) and its local names bound by an assignment (also annotated with a
      value, tuple and list targets, starred targets, `:=`), by the target of a `for` loop or by
      `with … as`. Each name is reported once per function, at its first binding, on the name
      itself.
    - **Outer scopes:** the enclosing functions, nearest first — their parameters and the names
      their bodies bind — and then the module. The module's names are those bound at its top
      level, also inside `if`, `for`, `while`, `try` and `with` blocks there: assignments
      (annotated ones only with a value), `for` and `with … as` targets, `def`, `class`, `import`
      (`import a.b` binds `a`; `as` gives the name; `from m import *` gives none). Where in the
      file the outer name is bound does not matter: a module variable assigned below the function
      counts.
    - **Never reported:** `self` and `cls`; names starting with `_`; names the function declares
      `global` or `nonlocal` (they are the outer name, not a new one); attribute and subscript
      targets (`self.x = …`, `d[k] = …`); augmented assignments (`x += 1`); names bound in a class
      body, whatever they are called; a name that only equals a name of an enclosing class body,
      because methods do not see that scope (`def value(self, value)` in a class is quiet unless
      the module binds `value`). Comprehensions and lambdas are not looked into.
    - **Message:** it names the kind of the reported name, the outer scope and the 1-based line of
      the outer binding: "Parameter 'item' shadows the name 'item' of the module (line 12)",
      "Local variable 'total' shadows the name 'total' of the enclosing function 'report' (line
      3)". The wording is IDECraft's own.
    - It is turned off or given another severity in the project's profile (behaviour 9,
      SPEC-0315) like every inspection; there is no suppression comment for Python (none of the
      Python rules has one). It has no quick fix: the fix is a rename (SPEC-0074).
13. **Own findings in `file_diagnostics` (v5):** the MCP tool `file_diagnostics` returns, after the
    language server's diagnostics, IDECraft's own findings of the file as the editor shows them
    (behaviour 3, after the profile, the suppressions and the project dictionary): `source`
    `IDECraft`, `code` the inspection id, `severity` `error`, `warning` or — for a weak warning —
    `information`, the range as 0-based lines and characters (Unicode scalar values), the message
    without a prefix, `unnecessary` for the unused rules. Findings of the level "no highlighting"
    are left out, as in the editor. The file is read from disk; a file over 1 MiB gets none. An
    agent so sees with one question what the editor underlines. A file without a usable language
    server is still answered with the error that names the server (SPEC-0023); `inspect_code`
    gives the own findings without one. [ADR-0008]
14. **Python: a method that does not use `self` (v6):** inspection
    `python.method-could-be-static` ("Method does not use 'self'", weak warning) reports a
    method that never touches its instance: it could be a `@staticmethod` or a function of the
    module, and a reader need not look for the state it depends on. The rule is kept narrow, so
    that what it reports is right. [OBS-0034 item 44, STD]
    - **Reported:** a `def` or `async def` that is a statement of a class body, whose first
      parameter is a plain `self` (no default, no star; an annotation is allowed) and whose
      body nowhere holds the name `self` — read, written, deleted, in a nested function, lambda
      or comprehension — nor a call of `super` nor the name `__class__` (both need the
      instance or its class). The finding stands on the method's name.
    - **Never reported:**
      - a method with any decorator (`@staticmethod`, `@classmethod`, `@property`,
        `@abstractmethod`, `@override`, or one IDECraft does not know: a decorator may pass
        the instance on);
      - a method whose name starts and ends with two underscores (`__init__`, `__eq__`): the
        language calls it on the instance;
      - a method whose body is only a docstring, `pass`, `...`, `raise NotImplementedError`
        (with or without a call) or several of these: a placeholder meant to be overridden;
      - a method of a class that names anything in its parentheses other than `object` — a
        base class, a `metaclass=` or another keyword: the method may override or implement
        one of a base class, or be called through it;
      - a method of a class whose name starts with `Test`: test runners call these on an
        instance, and `self` is the convention there;
      - a method whose first parameter has another name than `self`;
      - a `def` nested in a method or inside an `if`/`try` block of the class body.
    - **Names starting with one underscore are reported** (decision): a private helper is the
      method least likely to be overridden from outside, so the hint is most likely right
      there. A class without base classes can still be subclassed elsewhere; the rule accepts
      that, as it is a weak warning.
    - **Message:** "Method 'area' does not use 'self'; it could be a static method or a
      function". The wording is IDECraft's own.
    - It follows the profile (behaviour 9, SPEC-0315) and appears in `inspect_code` and
      `file_diagnostics` (behaviour 13) like `python.shadows-name`. Its quick fix is
      behaviour 15 (v7; none before).
15. **Python: quick fix "Make method static" (v7):** a finding of
    `python.method-could-be-static` carries the fix "Make method static", offered where every
    quick fix of an own inspection is (SPEC-0125: ⌥↩ on the finding, the Inspect Code results,
    fix all in file, MCP `fix_inspection` with its dry run as the preview). It makes two edits
    in the file and none elsewhere: [decision, STD, SPEC-0125]
    - **Decorator:** a line `@staticmethod` is put directly above the `def` (above `async def`
      for an asynchronous method) with the method's own indentation, spaces or tabs as written,
      and the file's line ending (`\r\n` when the file uses it).
    - **Parameter:** `self` leaves the parameter list with its annotation, the comma after it
      and the spaces or tabs after that comma: `(self, a)` becomes `(a)`, `(self: 'Shape',
      path)` becomes `(path)`. When `self` was the only parameter the parentheses are left
      empty, whatever spaces or trailing comma stood in them (`( self )` and `(self,)` become
      `()`). In a list written over several lines, a line that holds only `self,` goes
      as a whole and the other lines stay as written; a comment after the comma stays.
    - **Call sites are not touched.** A static method is called through an instance exactly as
      before (`shape.area()`, `self.area()` inside the class), and the instance is simply no
      longer passed; so the calls of the usual form keep working without an edit, and the fix
      needs no search of the project.
    - **No fix is offered** (the finding stays, without a fix) when the change would not be
      safe or cannot be made cleanly:
      - the file itself names the method through its class (`Shape.area`): a call of that
        form passes the instance by hand (`Shape.area(shape)`) and would then pass one
        argument too many. Other files are not searched; such a call in another file is the
        one case the fix can break, and the dry run and undo are the user's check;
      - `self` is followed by the positional-only marker `/` (`def f(self, /, a)`): removing
        `self` alone would leave a list that starts with `/`, which is not valid, and
        removing the marker would change how the other parameters may be passed;
      - the `def` does not start its line, or the method has a syntax error.
    - Cases that cannot occur, because behaviour 14 reports none of them: a method that
      already has a decorator, and a method whose first parameter has another name than `self`
      (`this`, `cls`) — such a method is not reported and so gets no fix either.
    - Fix all in file applies the fix to every reported method of the file; the edits of two
      methods never overlap.
16. **Only the project's own sources (v8; decision of the Project Owner of 2026-11-12: Inspect Code
    follows the rule Rename, Find in Files and Search Everywhere got):**
    - **Whole project:** a run over the whole project (the scope "Whole project" of behaviour 7,
      `inspect_code` without `path`) inspects the files that are the project's own by the rule of
      SPEC-0148 v4 behaviour 7 (`OwnSources`) — not virtual environments, installed dependencies,
      build output and caches, version control and IDE data, folders the project model excludes,
      anything behind a symbolic link, anything the ignore rules of Git match unless Git tracks
      it. Findings in a library are nobody's to fix and bury the project's own.
    - **A named place wins:** a file or folder named on purpose (the scopes "Current file" and
      "Folder of the current file", `inspect_code` with `path`, "Run inspection on…" of behaviour
      10 with such a scope) is inspected whatever it is: a file in `.venv`, every file below
      `node_modules/pkg`. A named folder that is the project's own is inspected by the rule (an
      ignored folder below it stays out). A path outside the project folder or behind a symbolic
      link is not read and gives no findings (security first).
    - **What was left out is said in one line:** `2 folders and 1 file not inspected (libraries
      and ignored files)` — the topmost folder or the single file each, version control data and
      `.idecraft` not counted, as in SPEC-0029 behaviour 10; for a named path that is not read,
      the path with the reason. The window shows the line under the results of the Inspection tab
      (also when nothing was found). `inspect_code` answers with an object: `findings` (the
      findings of behaviour 5, as before) and, when something was left out, `not_inspected` (that
      line with the hint to name a file or folder as `path`). Until v7 the answer was the bare
      list of findings; a second text next to the list would have changed its form only in
      projects with libraries, which is worse for whoever reads it than one form for all.
    - The reference index of the global inspection (behaviour 11, SPEC-0316) still reads every
      workspace file: a use in a library counts as a use. Findings are reported only for the
      files of the run. [GEN; decision]

17. **Python quick fixes of IDECraft's own (v9, OBS-0034 part 14, F-033):** pyright offers no quick
    fix at its "is not accessed" and "is not defined" diagnostics, so ⌥↩ offered nothing there.
    IDECraft finds these places itself from the syntax tree and the scopes of the file (the
    binding rules of SPEC-0330, "Naming and binding" of The Python Language Reference), without a
    language server. Decisions, each the safer variant:
    - **Shown nowhere by default:** the three inspections `python.unused-import`,
      `python.unused-variable` and `python.unresolved-name` have the level "No highlighting,
      only fix" (SPEC-0315 v2 behaviour 7). pyright already marks the places, and a place IDECraft
      would get wrong is never shown as a problem; the fixes are in ⌥↩ at the place. The level
      can be raised in the inspection profile.
    - **Left out where the syntax does not tell:** a file with `import *`, `globals()`,
      `locals()`, `vars()`, `exec(`, `eval(`, `__getattr__` or `__builtins__` gets none of them.
    - Each fix is one undo step on the editor's unsaved text; ⌥↩ previews its changed lines, MCP
      `fix_inspection` previews with `dry_run` (default) and writes with `dry_run: false`. A
      place with two fixes is two findings of the same id; `fix_inspection` takes the `title` of
      the one to apply (default the first) and names the others when it does not exist.
    - **Unused import (Q3, `python.unused-import`):** a name bound by an import statement of the
      module that nothing in the file reads, not in nested functions either, and not named in a
      string (`__all__`, a string annotation) or a `# type:` comment; not in a package's
      `__init__.py`. Fixes: "Remove unused import" removes the name, or the statement when no
      name is left, by the rules of Optimize Imports without ruff (SPEC-0329 behaviour 6:
      statements inside a block, sharing a line or holding a comment stay — then only the second
      fix is offered); "Optimize imports" runs Optimize Imports on the file (SPEC-0329), in MCP
      `optimize_imports` with the same `dry_run`. [OBS-0034 item 61, GEN]
18. **Local variable never read (Q2, `python.unused-variable`):** `name = expression` in a function
    (a single name, no chain) whose name the function never reads — not later, not by `+=`, not
    in a nested function, loop or comprehension. Left out: names starting with `_`, parameters,
    `global` and `nonlocal` names, module level (another module may read it), tuple targets.
    Fixes: "Remove assignment target" keeps the expression as a statement (`name = save(a)` →
    `save(a)`), so whatever it does still happens; "Remove statement" only for a value that
    cannot do anything — literals, tuples, lists, sets and dictionaries of literals, a negative
    number, a `lambda` — and when the statement stands alone on its lines; a block it leaves
    empty gets `pass`. A name, an attribute, a subscript, an operator or a call can run code or
    raise, so their statement stays (stricter than the rule of Inline Variable, SPEC-0330). "Add type hint"
    of the reference is not offered. [OBS-0034 item 60, GEN]
19. **Name not defined in a function (Q1, `python.unresolved-name`):** a name a function reads that
    no scope defines: not the function, an enclosing function, an enclosing class body (a class
    attribute read in a method is left alone — no report rather than a wrong one), the module
    (also further down, conditionally, by an import) and not Python's built-in names (3.12 and
    3.13, also `__name__`, `__file__` and the other module names). Fix: "Create parameter
    'NAME'" appends the name to the function's parameters, only when they are plain names (with or
    without a type); with a default value, `*`, `/`, `*args` or `**kwargs` a parameter after the
    others would take an argument a caller passes to another one, so no fix is offered there.
    Limit: callers are not changed; a call that does not pass the new argument fails with
    `TypeError` until it does. **Not offered:** "Create variable" — `name = None` before the
    line would turn the `NameError` into a program that runs with a value nobody chose; "Rename
    reference" and the reference's "Ignore …" entries. [OBS-0034 item 58, GEN]
20. **Inspect the current file (v10):** `CodeInspection.OnEditor` (⌥⇧I in both bundled keymaps;
    Code › Inspect Current File) runs Inspect Code over the active file at once, without the
    scope dialog, as the scope "Current file" of behaviour 7 does; the results replace the
    Problems window's Inspection tab. Without an active file the status bar says "Inspect
    Current File needs a file open in the editor." MCP: `inspect_code` with the file's `path`.
    [OSS: id and key of the bundled keymaps; GEN]
21. **Run inspection by name (v10):** `RunInspection` (⌥⇧⌘I, Ctrl+Alt+Shift+I; Code › Run
    Inspection by Name…) lists every inspection of the catalogue (behaviour 15's settings page
    has the same ones), grouped under their language and filtered by what is typed; the chosen
    one opens the scope dialog for that inspection only, as "Run inspection on…" of a finding
    does (v4 behaviour 10), also when the profile turns it off. Escape closes the list. MCP:
    `inspect_code` with `id`. [OSS: id and key of the bundled keymaps; GEN]

22. **Java: an anonymous class that can be a lambda (v11, OBS-0050 W3):** inspection
    `java.anonymous-to-lambda` ("Anonymous class can be a lambda", weak warning; short name
    `Convert2Lambda`) reports `new T() { … }` when all of these hold, on the text from `new` to
    the closing parenthesis of the arguments: [OBS-0050, STD]
    - `T` is a functional interface IDECraft knows without types: one of `Runnable`, `Callable`,
      `Comparator`, `ThreadFactory`, `FileFilter`, `FilenameFilter`, `PathMatcher`,
      `UncaughtExceptionHandler`, `ActionListener`, `PropertyChangeListener` or an interface of
      `java.util.function` (simple or qualified name), or an interface declared in the same file
      that declares exactly one method without a body and no `default` or `static` one;
    - the creation has no arguments and the body has exactly one member, a method that is not
      generic and not `static`;
    - the method's body does not use `this` or `super`, does not call the method by its own name
      (in a lambda that would name an outer method), calls without a qualifier only methods that
      a class around it in the file declares (any other could be a method of the interface,
      which the lambda no longer sees), and declares no parameter or local whose name the
      enclosing member already declares (a lambda cannot hide them).
    **Quick fix "Replace with lambda":** the whole creation becomes `(a, b) -> …` with the
    method's parameter names (one parameter without parentheses, none as `()`), and the body
    becomes the simplest form at once (IMP-0411): a body with only `return e;` or only one
    expression statement becomes the expression `e`, and when that expression is a call that
    behaviour 24 accepts, the lambda becomes the method reference itself. Otherwise the block is
    kept; either way the lines move left by the indentation the anonymous class added.
    Comments in a body keep the block. When the creation is an argument of `submit`,
    `schedule`, `invokeAll` or `invokeAny`, whose overloads take a lambda that returns nothing
    and one that returns a value, an expression statement keeps its block and no method
    reference is formed, so the call keeps its overload.
23. **Java: a block lambda with one statement (v11, OBS-0050 W4):** inspection
    `java.statement-lambda` ("Statement lambda can be an expression lambda", weak warning; short
    name `CodeBlock2Expr`) reports a lambda whose body is a block holding exactly one statement,
    `return e;` or an expression statement `e;`, and no comment, on the text from the parameters
    to the arrow; an expression statement is not reported in an argument of `submit`,
    `schedule`, `invokeAll` or `invokeAny` (behaviour 22). **Quick fix "Replace with expression lambda":** the block becomes `e`; the
    message names the result when it is short (up to 40 characters). [OBS-0050, STD]
24. **Java: a lambda that can be a method reference (v11, OBS-0050 W5):** inspection
    `java.lambda-to-method-ref` ("Lambda can be a method reference", weak warning; short name
    `Convert2MethodRef`) reports, on the call, a lambda whose body (as an expression or a block
    with only that statement) is one of: [OBS-0050, STD]
    - `q.m(p1, …, pn)` with exactly the lambda's parameters as arguments in order, where the
      qualifier `q` is `this`, `super`, a name or a chain of names (`System.out`) that names none
      of the parameters → `q::m`; a qualifier with a call, an index or a `new` is not reported,
      because the reference would evaluate it once instead of on every call;
    - `m(p1, …, pn)` without qualifier where `m` is a method of the enclosing class → `this::m`,
      or `C::m` when that method is `static` (`C` the enclosing class);
    - `p1.m(p2, …, pn)` where the first parameter has a declared type `T` in the lambda's
      parameter list → `T::m` (with inferred parameter types the type is not known from the
      syntax, so nothing is reported);
    - `new C(p1, …, pn)` with a class name `C` (with or without `<>`) and no body → `C::new`.
    Not reported: lambdas that are arguments of `submit`, `schedule`, `invokeAll` or
    `invokeAny` (behaviour 22), calls with explicit type arguments, a lambda whose parameters are used again in
    the arguments in another form (`x -> f(x, x)`), and calls inside the arguments. The message
    names the reference (`can be System.out::println`). **Quick fix "Replace with method
    reference":** the whole lambda becomes that reference.
25. **Levels (v11, IMP-0410):** the three inspections of behaviours 22–24 rewrite correct code
    into a shorter form; they are weak warnings, so the warning count of a file and of the editor
    (SPEC-0070) counts what is likely wrong and they are listed after the warnings. The profile
    (behaviour 9) can raise them to warnings. Agents get them from `inspect_code` and fix them
    with `fix_inspection` like every other inspection (SPEC-0125).
26. **Unused declarations in the editor (v12, OBS-0050 W1, W2, W6, W7):** the editor of a Java
    file of the project shows the findings of `java.unused-declaration` (SPEC-0316 v3) of that
    file, as Inspect Code would report them for it, with the look of the other unused findings
    (greyed text, a warning in the counter; "IDECraft: Method 'getCount()' is never used").
    [OBS-0050, GEN]
    - **From the project index, off the UI thread.** The project window refreshes the reference
      index (SPEC-0316 behaviour 2) on a background thread when it opens its first Java file, after
      every save of an indexed file (SPEC-0316 behaviour 1) and with every Inspect Code run; at the
      end of a refresh every open Java editor computes its findings again. Until the first refresh
      has ended the index is **not ready**: the editor shows none of these findings rather than
      findings from a partial index.
    - **After an edit.** The file's own declarations and uses come from the editor's current
      syntax tree, so unsaved text counts; the uses in the other files come from the index, that is
      from their saved state. The step starts 300 ms after the re-parse has installed a tree that
      matches the text, on a background thread; the main thread only installs its result. An edit
      cancels a running step (it stops at its next check, its result is dropped) and a result for a
      text that has changed since is never shown; until the new result arrives the shown findings
      move with the edits like the other problems (SPEC-0070). Typing never waits for the step.
      Budget: SPEC-0260 v4 (`editor_unused_ms_p95`, ≤ 500 ms for a 100 000-line Java file against
      the index of `java-large`).
    - **Not in the editor:** top-level types. The reference did not mark the public class `Fold`
      that nothing uses (OBS-0050); a top-level type is the file itself and is often reached from
      outside the sources (a launcher, a build script, reflection). Inspect Code and `inspect_code`
      still report them (SPEC-0316 behaviour 5).
    - **Rules as everywhere:** the entry points of SPEC-0316 v3 behaviour 4 are never reported;
      the profile turns the inspection off or changes its level (behaviour 9; "no highlighting,
      only fix" keeps the finding for ⌥↩ only), the suppressions of SPEC-0125 v2 behaviour 7
      apply, and the file's highlighting level (SPEC-0070 v6 behaviour 11) applies.
    - **Quick fixes (⌥↩ on the finding):** "Safe delete 'getCount'" runs Safe Delete (SPEC-0095 v12
      behaviour 15) on the declaration — the search for usages first, the deletion as one undo step
      when there is none, the list of usages with Cancel selected otherwise; ⌥⇧↩ runs it too. The
      finding's options are those of SPEC-0125 v2 behaviour 8: Suppress for statement (a
      `//noinspection unused` line above the member), for method, for class, all inspections for
      class, Run inspection on…, Disable inspection, Edit inspection settings. There is no "Fix all
      in file": every deletion is checked on its own.
    - **Not covered:** Kotlin files (the index holds no Kotlin declarations, SPEC-0316 out of
      scope; Kotlin files count as uses of Java names; v13: covered, behaviour 27), remote documents (the index is local), and
      changes made outside the window until the next refresh (a save in the window, Inspect Code, a
      new window). An unsaved use in another open editor counts once that editor is saved.
    - **Agents:** `inspect_code` and `reference_index` report the inspection as before, top-level
      types included; nothing changes for them. [ADR-0008]
27. **Unused declarations of Kotlin files in the editor (v13):** the editor of a Kotlin file (`.kt`)
    of the project shows the findings of `kotlin.unused-declaration` (SPEC-0316 v4 behaviours 9 and
    10) of that file the way behaviour 26 shows Java's: from the same reference index, with the
    same readiness, the 300 ms rest after the re-parse, the background step that an edit cancels,
    the same budget (SPEC-0260 v5, `editor_unused_kotlin_ms_p95`) and the same look
    ("IDECraft: Function 'helper' is never used"). The project window builds the index when it
    opens its first Java **or Kotlin** file; a refresh lets every open Java and Kotlin editor
    check again. Uses count across the languages both ways (SPEC-0316 v4 behaviour 9): a Kotlin
    function that only Java calls is not reported, nor a Java method that only Kotlin calls. [GEN]
    - **Not in the editor:** top-level classes, interfaces and objects (as behaviour 26); top-level
      functions and properties are reported, they are members of the file's class.
    - **Rules as everywhere:** the Kotlin entry points of SPEC-0316 v4 behaviour 10 are never
      reported; the profile, the suppressions of behaviour 28 and the highlighting level apply.
    - **Quick fixes (⌥↩ on the finding):** "Safe delete 'helper'" runs Safe Delete (SPEC-0095 v12
      behaviour 15) as in behaviour 26, ⌥⇧↩ too. The options of the finding are Suppress for
      declaration, Suppress for class, Suppress for file (behaviour 28), Run inspection on…,
      Disable inspection and Edit inspection settings; no "Fix all in file", no "Suppress all
      inspections for class".
    - **Not covered:** Kotlin scripts (`.kts`, they stay word files of SPEC-0316 behaviour 1),
      remote documents, and what behaviour 26 leaves out.
28. **Suppressing IDECraft's Kotlin findings (v13):** every own finding of a Kotlin file
    (`kotlin.*`, SPEC-0140, behaviour 27) is left out when the declaration it lies in, or a
    declaration around that one, carries `@Suppress("…")` or `@kotlin.Suppress("…")` with a
    matching name, or the file carries `@file:Suppress("…")`. A name matches as in SPEC-0125 v2
    behaviour 7: the id, the short name, `unused` for the unused rules, `ALL`. Short names:
    `unused` for `kotlin.unused-declaration` and `kotlin.unused-private`, `UNUSED_PARAMETER` for
    `kotlin.unused-parameter`. The options of a finding in a Kotlin file are [OSS, STD, GEN]
    - **Suppress for declaration:** `@Suppress("unused")` on its own line above the nearest
      declaration around the finding (class, object, companion object, function, property,
      constructor property, secondary constructor), indented like it; a `@Suppress(…)` already
      there gets the name added (`@Suppress("x", "unused")`);
    - **Suppress for class:** the same on the nearest class, interface or object around it;
    - **Suppress for file:** `@file:Suppress("unused")` as the first line of the file, after a
      leading comment block, before `package`; an existing `@file:Suppress(…)` gets the name
      added.
    Each option is offered only where it applies (not "for class" outside a class) and does
    nothing when the place already suppresses the finding. The name written is the short name,
    else the id. Agents: `suppress_inspection` takes the scopes `declaration` and `file` for
    Kotlin files besides `class` (`statement` is the same as `declaration` there). [ADR-0008]

## Acceptance criteria
- **AK-1** Every inspection reports its case and stays quiet on the near miss; findings are placed by
  line and column (unit tests `inspections::tests`).
- **AK-2** MCP: `inspect_code` over the project and over a folder (`crates/mcp/tests/it/protocol.rs`:
  `inspect_code_tool`).
- **AK-3** In a real window (`idecraft ui --selftest`): an empty catch block in a Java file appears among
  the editor's problems; Inspect Code lists it under its file on the Inspection tab.
- **AK-4** By hand: Inspect Code on a real project next to the reference.
- **AK-5 (v3)** Every inspection has a description of at least two sentences (unit test
  `inspections::tests::every_inspection_is_described`); the results tree and its order, the scopes,
  fix all over the listed findings (unit tests `problems_model::tests::inspection_tree_*`); a disabled
  inspection is not run and a changed severity is applied (unit test); window self-test: the dialog
  opens, Enter runs over the project, the tree shows the inspection node of the empty catch; side by
  side in OBS-0019 (re-check).

- **AK-6 (v4)** `inspect_project_only` runs one inspection, also when turned off, and the tab name
  (unit tests `inspections::tests::one_inspection_on_a_scope`,
  `problems_model::tests::inspection_tab_names`); MCP `inspect_code` with `id`
  (`crates/mcp/tests/it/protocol.rs`: `inspect_code_tool`); global inspection as in SPEC-0316 AK-3.
- **AK-7 (v5, behaviour 12)** A parameter and a local variable with the name of a module-level
  name, also one bound below the function, by an import, a `def` or inside an `if` block; a name
  of an enclosing function, nearest scope first; once per function at the first binding; tuple,
  `for`, `with … as` and `:=` targets; quiet for `self`/`cls`, a leading `_`, `global`/`nonlocal`
  names, attribute and subscript targets, augmented assignments, class body names, a method
  parameter that equals a class attribute, comprehensions and lambdas; the message with kind,
  scope and line (unit tests `python_inspections::tests`); listed in the catalogue with a
  description, turned off by the profile (unit tests `inspections::tests`).
- **AK-8 (v5, behaviour 13)** Findings as diagnostics: severity, source, code, range in characters
  on a line with non-ASCII text, "no highlighting" left out (unit test
  `inspections::tests::findings_as_diagnostics`); `idecraft call <folder> file_diagnostics` on a
  Python file lists pyright's diagnostics and `python.shadows-name`; `inspect_code` lists it
  without a server (`crates/mcp/tests/it/protocol.rs`: `inspect_code_tool`).
- **AK-9 (v6, behaviour 14)** A method that does not use `self`, also an `async` one, one with
  an annotated `self` and one starting with `_`, on its name; quiet when `self` is read,
  assigned to, used in a nested function, a lambda or a comprehension, with `super()` or
  `__class__`, with any decorator, for dunder methods, placeholder bodies, classes with a base
  class or a `metaclass`, `Test…` classes, another first parameter, a `def` nested in a method;
  `class A(object)` counts as without base (unit tests `python_inspections::tests`); listed in
  the catalogue with a description, weak, turned off and raised by the profile (unit tests
  `inspections::tests`); `inspect_code` without a server (`crates/mcp/tests/it/protocol.rs`:
  `inspect_code_tool`); `idecraft call <folder> file_diagnostics` on a Python file.
- **AK-10 (v7, behaviour 15)** The fix "Make method static" on a method with `self` alone, with
  further parameters, with an annotated `self`, on an `async def`, with tabs as indentation,
  with `\r\n` line endings, with a parameter list over several lines and with spaces or a
  trailing comma around a lone `self`; the result parses and is no longer reported; no fix
  when the file names the method through its class, when `/` follows `self`; fix all in file
  over several methods; the fix among the fixes at the caret, which is what ⌥↩ lists (unit tests
  `python_inspections::tests`, `inspections::tests`); MCP: `inspect_code` names the fix and
  `fix_inspection` previews and writes it (`crates/mcp/tests/it/protocol.rs`:
  `inspect_code_tool`); `idecraft call <folder> fix_inspection` on a Python file. In the
  window by hand: ⌥↩ on the method name.
- **AK-11 (v8, behaviour 16)** In a project with `.venv/lib/python3.12/site-packages/x.py`,
  `node_modules/y.js`, a file ignored by `.gitignore` and an own file, each with a finding: the
  whole-project run reports only the own file's finding and the line `2 folders and 1 file not
  inspected (libraries and ignored files)`; a run on `.venv` reports the finding of `x.py`, a run on
  the ignored file its finding, both without a line; a path behind a symbolic link gives no finding
  and the line names it (unit test `inspections::tests::a_project_run_inspects_the_own_sources`);
  MCP `inspect_code` the same, with `findings` and `not_inspected` (`crates/mcp/tests/it/protocol.rs`:
  `inspect_code_inspects_the_projects_own_sources`). In the window by hand: the line under the
  results.

- **AK-12 (v9, behaviours 17–19)** Unit tests `python_quick_fixes::tests::{unused_imports_are_removed_or_optimized,
  unused_locals_lose_their_target_or_their_statement, names_not_defined_can_become_parameters,
  findings_carry_only_fixes}`: each fix's text, every refusal and every name that is not
  reported. With `python3` (`crates/languages/tests/it/python_quick_fixes.rs`): removing an unused
  import, an assignment target (also of a call that has an effect) and an inert statement leaves
  the output unchanged; after "Create parameter" the function runs with the new argument; the
  file of OBS-0034 part 14 has its fixes at all three places. MCP
  (`protocol.rs::python_quick_fix_tools`): `inspect_code` names the six fixes at level `info`,
  `fix_inspection` previews the second fix by `title`, refuses an unknown title, writes "Create
  parameter", and "Optimize imports" answers as `optimize_imports`. In the window by hand: ⌥↩ at
  the three places of `probe.py`. Open.
- **AK-13 (v10, behaviours 20 and 21)** In a window of the test harness: ⌥⇧I on a Java file shows
  its findings in the Inspection tab with the scope of that file and no dialog; Run Inspection
  by Name lists the catalogue with language headings, narrows it by typing, and the chosen
  inspection opens the scope dialog for it alone; without a file the status bar says so
  (`eighth_package_keymap.rs::inspections_run_on_the_file_and_by_name`).

- **AK-14 (v11, behaviours 22–25)** Unit tests in `java_inspections::tests`:
  `anonymous_classes_that_can_be_lambdas` (each condition of behaviour 22 has a quiet near miss;
  the fix gives `() -> …` with an expression body, a method reference, and a kept block moved
  left), `block_lambdas_with_one_statement` (return and expression statements, quiet with two
  statements or a comment), `lambdas_that_can_be_method_references` (the four forms, quiet for a
  qualifier with a call, a repeated parameter, inferred types and type arguments). The text of
  the `Fold.java` of OBS-0050 gives exactly one finding of each of the three ids at lines 37, 40
  and 41 and no warning (`java_inspections::tests::the_fold_file_of_the_observation`).
- **AK-15 (v11)** In a window of the test harness, a `Fold.java` like the one of OBS-0050 shows
  three weak warnings and no warning in the editor's counter, and ⌥↩ on the anonymous class
  applies "Replace with lambda" as one undo step (`crates/uidriver/tests/it/scans.rs`:
  `java_lambda_simplifications_in_the_window`). MCP `inspect_code` on that file reports the three
  ids (`crates/mcp/tests/it/protocol.rs`: `inspect_code_reports_lambda_simplifications`).
  (v12: once the reference index is ready the counter also shows the four unused declarations
  of behaviour 26; the window test of AK-15 counts the errors and weak warnings only.)
- **AK-16 (v12, behaviour 26)** Unit tests `reference_index::tests::{editor_findings_of_the_unsaved_text,
  editor_findings_wait_for_the_index_and_stop_when_cancelled}`: the unused members of a file
  from its current text against the uses of the other indexed files, the top-level type left
  out, a use added in the text removes the finding, none before the index is ready, none after
  cancellation, the profile and a suppression. Window test
  (`crates/workbench/tests/it/java_unused_window.rs::unused_declarations_appear_in_the_editor`): a
  file like OBS-0050's `Fold.java` with a `main` class next to it shows exactly the four warnings
  `getCount()`, `task`, `Inner` and `sizes` and none for the entry points of the fixture (`main`,
  an annotated method, `toString`, a `serialVersionUID`, the top-level class); a use typed into the
  file removes the warning of `getCount()`; ⌥↩ on `task` lists "Safe delete 'task'" and the
  Suppress options. Benchmark `editor_unused_ms_p95` within SPEC-0260 v4.
- **AK-17 (v13, behaviours 27 and 28)** Unit tests
  `reference_index::tests::{kotlin_declarations_and_entry_points, uses_across_java_and_kotlin,
  kotlin_editor_findings}`: the unused Kotlin members of a file from its current text, a Kotlin
  function used only from Java and a Java method used only from Kotlin not reported, a property
  used through its Java getter not reported, the entry points of SPEC-0316 v4 behaviour 10 not
  reported, the top-level class left out; `kotlin_inspections::tests::suppress_kotlin_findings`:
  `@Suppress` on the declaration, on a class around it and `@file:Suppress` leave the findings
  out, the three options write the annotations and add a name to an existing one. Window test
  (`crates/workbench/tests/it/kotlin_unused_window.rs::kotlin_unused_declarations_appear_in_the_editor`):
  a Kotlin file next to a Java file shows exactly the warnings of the fixture's unused function,
  property and nested class, none for the entry points and none for the function Java calls; ⌥↩
  on a finding lists "Safe delete 'name'" first and the three Suppress options, "Suppress for
  declaration" writes `@Suppress("unused")` and the warning goes. Benchmark
  `editor_unused_kotlin_ms_p95` within SPEC-0260 v5, `editor_unused_ms_p95` still within its
  budget.

## Out of scope
v11: unused declarations that are not private (OBS-0050 W1, W2, W6, W7) stay a global inspection
of Inspect Code (behaviour 11, SPEC-0316) and are not shown while typing; the reference shows them
in the editor of a one-file project (v12: shown in the editor, behaviour 26). Converting a lambda back into an anonymous class, a method
reference into a lambda, and lambdas whose parameter types would be needed (`s -> s.length()`
without a declared type) are not offered.
Looking at subclasses in other files before reporting (v6); v7: searching other files for calls
of the method through its class before offering the fix, rewriting such calls, turning the method
into a function of the module or into a `@classmethod`. Names bound by `except … as`, by an `import` inside a function, by a nested `def` or `class`, by
`match` patterns, by comprehensions and lambdas (v5); shadowing of built-in names (`list`, `id`),
which the reference reports with a rule of its own; a rename offered as quick fix. The reference's hundreds of inspections, custom named scopes, its language and category levels in
the tree, injected code, language-server diagnostics in the results, export and offline inspection
reports.

## Known deviations from the reference
The reference's inspection set is far larger and configurable; IDECraft starts with a small set of
rules that the language servers miss. v3: compared side by side in OBS-0019; the tree groups by inspection without the reference's
language and category levels.
v5: the shadowing rule was seen in the reference on one case only (OBS-0034 item 33: a parameter
with the name of a module variable); which other bindings the reference reports was not observed,
so IDECraft's set follows the language's binding rules and stays with parameters, assignments,
loop and `with` targets. The message is worded by IDECraft and adds the outer binding's line.
v11 (OBS-0050): the three lambda simplifications are weak warnings instead of warnings (IMP-0410),
so a file such as the observation's counts zero warnings and three weak warnings where the
reference counts seven warnings (four of them unused declarations, see Out of scope). The fix of
an anonymous class gives the simplest form in one step and the messages name the result
(IMP-0411); the reference needs one fix per layer.
v12: the reference marks unused non-private declarations in the editor of a one-file project
(OBS-0050); whether it does so in a project of many files was not seen. IDECraft shows them in
every project from the reference index, with uses counted by name (SPEC-0316 known deviations),
so a name used anywhere keeps its declarations quiet.
Improvement: every inspection is also available to agents with machine-readable ids and positions, and
runs headless (IMP-0035).
