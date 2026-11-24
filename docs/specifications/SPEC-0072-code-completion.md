# SPEC-0072: Code completion in the editor

- **Version:** 21 · **Date:** 2026-11-19 (v21: among the names the server ranks alike, those declared in the file come first, then those declared in the project, and in a call position the callables; sort text made of a class and the label counts as that class; behaviours 56 and 57, AK-42; side-by-side comparison of 2026-11-19, F-038 gap PH1); 2026-11-18 (v20: the documentation of the item selected when the list opens or after it narrows is shown at once, behaviour 13, AK-41; side-by-side comparison of 2026-11-18, F-036/F-031 gap TC1); 2026-11-18 (v19: Tab or Enter on the name list of a template's place takes the name and goes on to the next place, behaviours 32 and 51; ⇧Tab at the first place stays in the template, behaviour 51; AK-40; OBS-0086 K2, K3); 2026-11-17 (v18: `complete_code` narrows by the typed prefix with the editor list's matcher and answers in pages, behaviour 55, AK-39; OBS-0081 L7; v17: the server's list is ranked before it is cut to the number asked for, behaviour 54, AK-38; headless check of F-038, OBS-0081); 2026-11-16 (v16 (v15 is taken by #878): Tab expands a Python live template named as typed, behaviour 53, AK-37 (behaviour 52 and AK-36 are taken by #878, AK-35 by #887); eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-16 (v15: in the `class` and `id` values of an HTML file the list opens while typing and the typed name includes `-`, behaviour 52, AK-36; OBS-0069 G1, with SPEC-0042 v22 behaviour 39); 2026-11-15 (v14: Insert Live Template, behaviour 50, and Next and Previous Template Variable, behaviour 51, AK-33 and AK-34; tenth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v13: Class name completion, behaviour 48, and inserting a suggestion with a dot, behaviour 49, AK-31 and AK-32; eighth package of docs/research/KEYMAP_COVERAGE.md; v12: the documentation panel prefers the right side of the list and
  stays inside the editor area on the left, behaviour 16, AK-35; second window check of 2026-11-14; v11: Enter and Tab of the list are the keymap actions
  `EditorChooseLookupItem` and `EditorChooseLookupItemReplace`, behaviour 47; v10 2026-11-12; v2: ⌥Esc on macOS, IMP-0032; v3: the list opens by itself while typing;
  v4: every row is one line, long details are cut off with an ellipsis; v5: icons, highlighted prefix,
  right-aligned type, server ranking, documentation beside the list, hint line; OBS-0012 side by side;
  v6: the signature gives way before the type, the documentation stays inside the window; OBS-0013;
  v7: overload order, snippets with tab stops, postfix and live templates from the server, imports,
  call parentheses, exact name first; OBS-0017; v8: smart type completion, Java documentation
  repaired, TypeScript types, parentheses and postfix templates, template icon, name suggestions, name
  stop first; defects F031-1 to F031-4 of OBS-0017; v9: Python — types from the resolved
  documentation, sort text ignoring case, the first list after a server start, call parentheses,
  postfix and live templates, standard library imports, overloads collapsed, a list asked again when
  the typed name no longer fits; OBS-0021; v10: Python — the class a member comes from, and
  `complete_code` with signature, type and origin without being asked; OBS-0034 L7)
- **Sources:** GEN (a list of suggestions under the caret that narrows while typing; F-031), OSS (action id
  `CodeCompletion` with Ctrl+Space in the ported keymaps), STD (LSP `textDocument/completion`, snippet
  syntax with tab stops), SPEC-0024 (completion from the language server), SPEC-0058 (editor view),
  OSS (action id `SmartTypeCompletion` with Ctrl+Shift+Space in the ported keymaps, the template icon
  `nodes/template` of `JetBrains/intellij-community`), OBS-0017 (defects F031-1 to F031-4), OBS-0021
  (Python), OSS (typeshed stubs of the standard library that ship with pyright, read at run time);
  v10: OBS-0034 item 36 (after `item.` the reference names for every member the class it comes
  from, `Item` or `object`; Ferris IDEA names none and `complete_code` leaves `detail` empty), STD
  (LSP `completionItem/resolve`, `labelDetails`, `textDocument/typeDefinition`,
  `textDocument/definition`; The Python Language Reference: attribute lookup follows the method
  resolution order, the C3 linearisation of the base classes, and every class without bases
  derives from `object`; `dataclasses`: the methods the decorator may add), GEN (tried with
  pyright 1.1.414: what its completion items carry, see behaviour 44); v17: OBS-0081 (headless on 2026-11-17:
  `complete_code` after `r.` in a Rust file listed `into()` and `try_into()` before the type's own `area()`, which
  rust-analyzer had preselected with a better sort text; in a PHP file the project's function `greet` was not among
  the first 100 of intelephense's items for `gr`), STD (LSP: a client sorts completion items by `sortText`, the
  label where it is missing; the order of the list as sent means nothing), OSS (rust-analyzer and intelephense
  send their lists in an order of their own, not by sort text); v15: OBS-0069 G1 (the
  reference lists the stylesheets' classes in an empty `class` value; Ferris IDEA listed nothing, by
  hand or while typing), STD (CSS identifiers hold `-`; HTML attribute values are quoted text);
  v18: OBS-0081 item L7 (after `<sec` the HTML server's list came from `complete_code` with
  `html`, `head`, `title` first and `section` further down; the window narrowed it, the tool did
  not)
- **Feature matrix rows:** F-031, F-038

## Purpose
Complete identifiers from the language server without leaving the keyboard, on the text as it is in the
editor.

## Behaviour
1. **Start:** `CodeCompletion` (Ctrl+Space) asks the file's language server in the background for
   completions at the caret, sending the editor's current text (also unsaved changes); the status bar shows
   "Completing…", or "No suggestions" when there are none. [GEN, OSS, STD]
2. **Prefix:** the identifier part (letters, digits, `_`) left of the caret is the typed prefix; it is
   replaced on acceptance. [GEN]
3. **Narrowing:** while typing, the list keeps the suggestions whose label starts with the prefix
   (ignoring case, shorter first), then fuzzy matches (SPEC-0031). It closes when nothing matches, when the
   caret leaves the identifier, or when another tab becomes active. [GEN]
4. **Accept:** Enter or Tab inserts the selected suggestion in place of the prefix; snippet tab stops are
   dropped and placeholders keep their default text (`${1:a}` → `a`). One undo step. [GEN, STD]
5. **Keys and look:** Up/Down move the selection with wrap-around, Escape closes. The list floats under the
   start of the identifier with kind, label (editor font) and detail; up to ten rows around the selection
   are shown. [GEN]
6. The same unsaved-text request is used by Go to Declaration (SPEC-0071), which therefore no longer needs
   the file saved. The MCP counterpart is `complete_code` (SPEC-0024). [ADR-0008]

7. **⌥Esc on macOS (v2):** on Macs with more than one keyboard layout, macOS takes ⌃Space for switching
   the input source and the IDE never sees it; the macOS keymap therefore also binds ⌥Esc (the Cocoa
   "complete" key, unused by the reference's keymaps) to `CodeCompletion`. ⌃Space keeps working where
   macOS leaves it alone. [GEN, improvement IMP-0032]

8. **v3 – Opens while typing:** 100 ms after the last typed character, when the caret in the focused
   editor follows an identifier (one that does not start with a digit) or a member access (`.` after an
   identifier, `)` or `]`; `->`; `::`), and the character before the caret is not in a comment or a
   string, the list opens by itself like behaviour 1 but without status messages; no suggestions means no
   list. A list already open stays. Typing on narrows it as in behaviour 3. [GEN]

9. **v4 – One line per row:** every row of the list is exactly one line high. Line breaks and runs of
   whitespace in a server's label or detail are shown as single spaces; the label keeps its width and the
   detail takes what is left of the row, cut off at the right edge with "…" when longer. Details never
   wrap, spill into the next row or lie over each other. [GEN; OBS-0012: long Java details overlapped in
   Ferris IDEA]

10. **v5 – Row layout:** a row shows, left to right: the kind as a 16 px symbol icon (the same icons as
   the Structure view, SPEC-0079; database items use the schema, table and column icons; kinds without an
   icon leave the place empty instead of naming the kind in words), the label in the editor font, the
   signature part of the label in grey right after it (LSP `labelDetails.detail`, e.g. `(E... elements)`),
   and, aligned to the right edge of the list, the type in grey (`labelDetails.description`, otherwise
   the detail). The type column is cut off with "…" when it does not fit, the label never is (v6:
   only after the signature, behaviour 15). The client announces label-detail support to the server. [GEN, STD; OBS-0012]
11. **v5 – Typed prefix highlighted:** the characters of the label that match the typed prefix (the
   leading characters for a prefix match, the matched characters for a fuzzy match) are drawn bold in the
   accent colour. [GEN; OBS-0012]
12. **v5 – Server ranking:** the list starts in the server's order: an item the server preselects first,
   then by the server's sort text, then the server's own order. Narrowing keeps that ranking among the
   prefix matches; items without sort text (plugins, SQL) are ranked shorter first as before; fuzzy
   matches follow. [STD; OBS-0012: the reference put the varargs overload of `List.of` first while
   Ferris IDEA sorted the shortest label first]
13. **v5 – Documentation beside the list:** next to the right edge of the list a panel shows the
   documentation of the selected item (Markdown rendered like the documentation popup, SPEC-0077) and
   follows the selection. Documentation that the server leaves out of the list is asked for with
   `completionItem/resolve` in the background, once per item; a selection that has none shows no panel.
   [STD; OBS-0012] (v20) This holds for every selected item, not only after moving the selection:
   the item selected when the list opens and the one selected after the list narrows by typing show
   their documentation as soon as it is known, without a key press. Signature and type that the
   server adds later for the same item (behaviour 28) do not make it a different item: the
   documentation already asked for, or brought along with them, stays shown. [Side-by-side
   comparison of 2026-11-18 with typescript-language-server: after `xs.` on `const xs = [1, 2, 3]`
   the reference showed the documentation of the first item `at` at once; Ferris IDEA showed it only
   after moving down and back up]
14. **v5 – Hint line and Tab:** the bottom of the list shows one grey line naming the two ways to accept,
   "⏎ insert  ·  ⇥ replace". Enter inserts in front of the rest of the identifier after the caret (as in
   behaviour 4); Tab replaces the prefix and the rest of the identifier after the caret. [GEN; OBS-0012]

15. **v6 – The type stays whole:** when a row is too narrow for label, signature and type, the
   signature gives way first: it is cut off with "…" (and left out when not even two characters
   fit), while the type at the right edge stays whole. Only when label and type alone do not fit is
   the type cut off with "…" as well; the label never is. Widths are counted in characters of the
   editor font, which the list uses. [GEN; OBS-0013: a long parameter list reduced the type to a
   fragment such as `<E>`]
16. **v6 – Documentation inside the window:** the list and its documentation panel stay inside the
   window with a margin of 8 px. The list moves left when it would cross the right edge. The panel
   (380 px wide) goes to the right of the list when there is room, else to its left; when neither
   side has 380 px, it takes the wider side and narrows to the room there (not below 160 px; with
   less room on both sides no panel is shown). Its text wraps at the panel's width, code lines
   included, so nothing is cut off at the window edge. [GEN; OBS-0013: the panel was cut off at the
   window edge]
   **v12 (second window check 2026-11-14):** the left side of the list ends at the editor area,
   not at the window: the panel never lies over a tool window left of the editor. In order: right
   of the list with its 380 px when they fit into the window; else left of the list with its
   380 px when they fit between the editor's left edge and the list; else narrowed on the side
   with more room — the right one when both have the same — and not below 160 px; else no panel.
   It never covers the list. Until v12 the left side reached to the window's edge: with the list
   near the left edge of the editor and less than 380 px to its right the panel was put over the
   Commit window. [GEN]

17. **v7 – Overloads:** suggestions with the same name that the server ranks alike are ordered: the one
   taking a variable number of arguments first (its label detail contains `...`), then fewer
   parameters first. [GEN; OBS-0017: the reference lists `of(E... elements)` before `of()`, `of(E e1)`, …]
18. **v7 – Snippets and tab stops:** the client announces snippet support. A suggestion in snippet form is
   inserted with its placeholders' default text (a choice: its first entry; a variable such as
   `$TM_SELECTED_TEXT`: its default or nothing); each further line of a multi-line snippet starts with
   the indentation of the line it goes into, and a tab in the snippet becomes the editor's indent unit.
   The caret goes to the first tab stop (`$1`, `$2`, …, `$0` last), its placeholder selected; a stop
   written in several places gets a caret in each, so typing changes all of them. While stops are
   left, Tab or Enter (without modifiers, no list open) selects the next one; Escape, leaving the
   inserted text or reaching the last stop ends this. Without stops the caret goes after the text.
   This brings the server's live templates (Java: `sout`, `fori`, `psvm`/`main`, …) and postfix
   templates (`.var`, `.if`, `.for`, `.nnull`, `.sysout`, …) into the list. [STD; OBS-0017]
19. **v7 – Edit range:** when the server's edit for a suggestion starts before the typed name (a postfix
   template covers its expression, e.g. `s.length().var`), accepting replaces from that start up to the
   caret (with Tab also the rest of the name). [STD; OBS-0017]
20. **v7 – Imports:** a suggestion's additional edits (an import of the chosen type or function) are
   applied together with it in one undo step. When the server sent none and resolves items, they are
   asked for with `completionItem/resolve` on the text as it was before accepting; edits that end
   before the inserted text are applied, joined into the same undo step, caret and tab stops moved
   along. Up to 5000 suggestions of an answer are kept, so names to import that a server lists late
   are not cut off. [STD; OBS-0017: TypeScript `bySku`, Java `ArrayList`, Python `restock`]
21. **v7 – Java:** jdtls is configured to insert method calls with empty parentheses and the caret
   inside (no guessed arguments) and to leave JDK-internal packages (`com.sun.*`, `sun.*`, `jdk.*`,
   `org.graalvm.*`) out of the list; postfix completion is on. [GEN; OBS-0017]
22. **v7 – Tab over a call:** when Tab replaces a name followed by `(`, the suggestion's own argument
   list is left out and the existing parentheses stay; the caret goes after `()` for a method without
   parameters, else after `(`. [GEN; OBS-0017: `s.is|Empty()` → Tab on `isBlank` → `s.isBlank()|`]
23. **v7 – Exact name first:** a suggestion whose name equals the typed name (ignoring case) comes
   before the other prefix matches. [GEN; OBS-0017: `names.for` lists the postfix `for` before
   `forEach`]
24. **v7 – Typed name after a member access:** when the server lists nothing for a name typed after
   `.`, the list is asked for without that name and narrowed here (prefix, then fuzzy), so `.sout`
   finds the postfix template the server calls `sysout`. [GEN; OBS-0017]

25. **v8 – Smart type completion:** `SmartTypeCompletion` (Ctrl+Shift+Space, both keymaps) finds the
   type expected at the caret and lists only suggestions of that type (F031-1). The expected type comes,
   first match wins, from (a) a declaration left of the caret, `Type name = |` (Java, C-like) or
   `name: Type = |` (TypeScript); (b) an assignment `name = |`: the type of `name` from the server's
   hover; (c) an argument `call(…, |`: the type of the parameter at the caret in the server's signature
   help, for every overload with enough parameters; (d) `return |`: the return type of the enclosing
   method from the document symbols (jdtls: detail ` : Type`). Without an expected type the status bar
   says so and no list opens. [GEN, STD; OBS-0017 step 4]
26. **v8 – What fits:** a suggestion's type is its label description (LSP `labelDetails.description`),
   else the part after the last ` : ` of its detail; TypeScript items without one are resolved first
   (behaviour 28, at most 100). A type fits when it is assignable as far as Ferris IDEA can tell without a
   compiler: equal after dropping package names and spaces; type variables (`E`, `T`, …) fit anything
   and type arguments are compared only when both sides name concrete types; Java primitive widening
   and boxing; `Object`, `any` and `unknown` take everything; a union takes each member; `T[]` equals
   `Array<T>`; supertypes of common JDK types (strings, numbers, collections, maps) from a built-in table
   and from the `extends`/`implements` clauses of the types declared in the open file. `true`/`false`
   fit `boolean`, `null` fits reference types. Java also asks the server for the members of the expected
   class (completion after `Type.`) and offers the static ones that return a fitting type with the class
   name: `List.of` is inserted as `List.of(|)`. Order: the same type before other fitting types, then the
   server's ranking. The hint line of the list names the expected type (IMP-0302). The MCP counterpart
   is `smart_complete_code`: the expected type, where it came from, and the items. [GEN; OBS-0017 step
   4: the reference lists `names` first, then the static factories of `List`]
27. **v8 – Java documentation repaired:** jdtls turns some Javadoc into broken Markdown (an inline
   `{@return …}` with nested tags gives stray `}` and links whose text runs into the next sentence;
   OBS-0017 step 11). When the documentation of a Java item has a `}` or `{@` outside code, Ferris IDEA
   renders the Javadoc itself from the source jdtls refers to: the JDK's `lib/src.zip` next to the
   `jrt-fs.jar` named in its `jdt://` links, or the project file of the declaring type. The comment
   before the member of that name (for methods: with that parameter count) is converted by an own
   converter: `{@code}` and `{@literal}` become code, `{@link}`/`{@linkplain}` their label (else the
   member's name), `{@return x}` "Returns x.", common HTML (`<p>`, `<br>`, lists, `<code>`, `<pre>`,
   `<b>`, `<i>`) its Markdown, block tags the labelled sections of SPEC-0077 (Parameters, Returns,
   Throws, Since, See Also). Without a source the broken text is cleaned up: `jdt://` link targets and
   stray braces are removed. [GEN; OBS-0017 step 11]
28. **v8 – Types from resolving:** rows that come without signature and type (typescript-language-server
   and pyright give them only when an item is resolved) are resolved in the background for the rows
   shown, once per item, in one server question for up to ten rows. The resolved detail gives signature
   and type: `(property) Item.qty: number` → type `number`; `function bySku<T>(items: T[]): Map<string,
   T>` → signature `(items: T[])`, type `Map<string, T>`; its documentation is kept for the panel. MCP
   `complete_code` does the same for the returned items with `details: true`. [STD; OBS-0017 step 9]
29. **v8 – Call parentheses (TypeScript, JavaScript):** a function or method is inserted with `()` and
   the caret inside, unless the text after the name already starts with `(` or the name is completed
   inside the braces of an `import`/`export` list. [GEN; OBS-0017 step 9: `bySku()` with the caret
   inside]
30. **v8 – Postfix templates (TypeScript, JavaScript):** typescript-language-server has none, so Ferris IDEA
   adds its own after `expression.`: `const`, `let`, `var` (a variable named by behaviour 32), `if`,
   `else` (`if (!x)`), `not` (`!x`), `null` (`if (x === null)`), `notnull` (`if (x !== null && x !==
   undefined)`), `return`, `for` (`for (const item of x)`), `forin`, `fori` (`for (let i = 0; i <
   x.length; i++)`), `log` (`console.log(x)`), `par` (`(x)`), `await` and `typeof`. The expression is
   the member-access chain before the dot on the line (names, `this`, calls, index brackets, `?.`, `!`).
   They follow the server's items when the typed name is a prefix of theirs and replace expression and
   name like behaviour 19, with tab stops. MCP `complete_code` returns them too. [GEN; OBS-0017 step 9:
   the reference lists `const`, `let`, `if`, … after `item.`]
31. **v8 – Template icon:** rows of kind snippet (live and postfix templates) show the template icon
   (`nodes/template`, the open New UI icon). [OSS; OBS-0017 step 6]
32. **v8 – Name suggestions at a placeholder:** when the selected tab stop names a new variable (an
   identifier placeholder followed by `=`, `:`, `of` or `in`, after a type, `const`, `let` or `var`), the
   list opens with suggested names; typing narrows it, Enter or Tab puts the chosen name in all places
   of the stop, and the next Tab goes on to the next stop. (v19: Enter or Tab on the list puts the
   name in and goes on to the next stop at once, behaviour 51.) Names come from the expression (the last call
   or field: `getName()` → `name`, `s.length()` → `length`; the singular of a plural collection: `names`
   → `name`, `entries` → `entry`), then from the type (`List<String>` → `strings`, `list`; `Item` →
   `item`; `int` → `i`; `String` → `s`, `str`), without keywords and duplicates. When the server's
   default is the collection's name with a digit (jdtls `.for`: `names2`), the first suggestion takes
   its place. The MCP counterpart is `suggest_names`. [GEN; OBS-0017 steps 6, 7]
33. **v8 – The name first:** in a snippet whose first stop is a type directly followed by the stop of
   the declared name (jdtls `fori`: `${1:int} ${2:i}`), Tab visits the name first, then the type, then
   the rest. [GEN; OBS-0017 step 7: the reference starts `fori` at `i`]

34. **v9 – Python types from the documentation:** pyright gives neither signature nor type with its
   items and puts the declaration only into the documentation of a resolved item. For Python the
   detail of a resolved item without one (or with only `Auto-import`) is the first declaration of the
   documentation's first code block, on one line and without an overload body: `qty: int` → type
   `int`; `def join(a: StrPath, /, *paths: StrPath) -> str` → signature `(a: StrPath, /, *paths:
   StrPath)`, type `str` (behaviour 28, with `->` for the return type). Classes keep no detail (their
   bases are no signature). [STD; OBS-0021 items 1, 4]
35. **v9 – Case in the ranking:** sort texts are compared ignoring case first (pyright's
   `09.9999.abort` before `09.9999.CLD_CONTINUED`, so `os.` lists `abort`, `access`, …,
   `AT_NO_AUTOMOUNT` as one alphabet); among names equal to the typed name ignoring case, the one in
   the typed case comes first (`Path` before `path` for `Path`). [GEN; OBS-0021 items 1, 8]
36. **v9 – A member access with a list open:** typing `.`, `>` or `:` while a list is open (the list of
   `os` while typing `os.`) closes that list and opens the list of the members by itself (behaviour
   8); an automatic answer that arrives for a text typed on meanwhile is dropped and asked again
   when the typed name no longer fits it. While a Python server has been running for less than 30 s
   and answers with an empty list, the list is asked for again every 0.5 s for up to 10 s (as for
   TypeScript). Deviation: where the reference's analysis stays silent, Ferris IDEA answers or names the
   cause (IMP-0316). [GEN; OBS-0021 item 1: `os.` typed right after opening showed no list]
37. **v9 – Python call parentheses:** a function or method is inserted as `name()` (snippet
   `name($0)`), unless the text after the name already starts with `(`, the name follows `def`,
   `class` or `@`, or the line is an `import`/`from` line; classes get none. When the resolved
   signature of a row takes no arguments (`()`, `(self)`, `(cls)`; also for TypeScript), the caret
   goes after the parentheses (`os.getcwd()|`), else inside, and parameter info opens as after typing
   `(` (SPEC-0080). [GEN; OBS-0021 items 2, 3, 7, 9]
38. **v9 – Python postfix templates:** after `expression.` (the expression as in behaviour 30, not
   `self` alone, not in import lines) Ferris IDEA adds its own templates, replacing expression and name
   with tab stops: `if` (`if x:`), `ifn` (`if x is None:`), `ifnn` (`if x is not None:`), `not`,
   `var` (`name = x`), `for` (`for item in x:`), `fori` (`for i, item in enumerate(x):`), `while`,
   `with` (`with x as name:`), `print`, `return`, `yield`, `raise`, `par`, `len`, `str`, `list`, `set`,
   `tuple`, `dict`, `isinstance` (`isinstance(x, object)`), `main` (`if __name__ == "__main__":` with
   `x` in the body). Bodies are indented by the editor's indent unit; variable names come from
   behaviour 32. The right column describes the result. [GEN; OBS-0021 items 4, 5]
39. **v9 – Python live templates:** a name standing alone at the start of a statement offers the
   templates it begins: `main` (only at the top level: `if __name__ == "__main__":` and the caret in
   the body), `iter` (`for item in iterable:`), `itere` (`for i, item in enumerate(iterable):`), `prop`
   (a read-only property) and `super` (`super().__init__()`); the template named exactly as typed is
   preselected; the documentation beside the list shows the template's text. [GEN; OBS-0021 item 6]
40. **v9 – Standard library names:** pyright offers imports only from modules it has already seen.
   Ferris IDEA reads the public top-level names (classes, functions, annotated or assigned names, not
   starting with `_`, not only inside `if` blocks) of the standard library stubs that come with the
   Python language server (`typeshed-fallback/stdlib` next to pyright or basedpyright; private modules
   and `builtins` left out) once per process. For a name of at least two characters, not after `.`,
   not in an import line, up to 50 names starting with it (ignoring case) are added that the server
   does not already list from the same module and that are not local: same case first, shorter
   first, shallower module first; each shows the module at the right, ranks like the server's own
   imports and carries the import as an additional edit (behaviour 20). [OSS; OBS-0021 item 8]
41. **v9 – Where the import goes:** a name is added to an existing single-line `from module import …`
   at the top of the file; else `from module import name` goes after the last standard library import
   at the top; else before the first import, followed by an empty line when that import is not from
   the standard library; else after a module docstring, followed by an empty line. [GEN; OBS-0021
   item 8: `from pathlib import Path` right after `import os`]
42. **v9 – Overloads collapsed:** in the documentation of a Python item the code block keeps only the
   first of several overloads (with its decorators), followed by a line naming how many more there
   are; the description after the block stays. [GEN; OBS-0021 item 11]
43. **v9 – A name that no longer fits the list:** servers filter the list by the name typed when it
   was asked for. When the typed name neither extends that name nor is a shorter part of it (the
   caret moved back into `item.qty` and `t` was typed: `item.t|qty`), the list closes and opens again
   for the new name by itself; any identifier character typed while a list is open asks for a new
   list when the open one closes. [GEN; OBS-0021 item 9: the reference lists `total_cents` first]
44. **v10 – The class a Python member comes from:** pyright and basedpyright do not say where a
   member is declared. Tried with pyright 1.1.414 after `item.`: an item carries its label, kind
   and sort text, no `detail` and no `labelDetails`; resolving it adds only the documentation,
   whose code block is the declaration (`name: str`, `def total() -> float`; without `self`, and
   `undefined` for the methods a dataclass generates; that word is no declaration and is not
   taken as a detail). The type and the parameters therefore stay
   those of behaviours 28 and 34, and the origin is worked out from answers of the server and the
   sources, never guessed: [STD, GEN; OBS-0034 item 36]
   - **When:** in a Python file whose server is pyright or basedpyright, for a member access: the
     name being completed follows a `.` that follows a name (`item.`, `self.items.na`). After a
     call, an index or a literal (`f().`, `xs[0].`, `"".`) no origin is given.
   - **The receiver's class:** the server is asked for the type definition of the name before the
     dot (`textDocument/typeDefinition`). Exactly one place that is the name of a `class`
     statement gives the class; no place, several places (a union) or another place (a module, a
     variable) give no origin for the list.
   - **What a class declares:** read from the class's source or stub with Ferris IDEA's syntax tree:
     the methods and nested classes of its body, the names assigned or annotated there (also
     inside `if`/`else` and `try` blocks of the body, as stubs have them), and the attributes its
     methods assign through their first parameter (`self.extra = 1`).
   - **Base classes:** each base written in the class statement (a name, a dotted name, or one
     with type arguments such as `Sequence[str]`) is looked up with `textDocument/definition` at
     its place and must again lead to exactly one `class` statement; keyword arguments
     (`metaclass=`) are skipped. `Generic` and `Protocol` declare no members and are left out. A
     class without bases derives from `object`, read from `builtins.pyi` of the typeshed stubs
     beside the server (behaviour 40). At most 32 classes are read.
   - **Origin of a name:** the first class in the method resolution order that declares it. With
     single inheritance that is the chain of bases, however far it could be followed; with several
     bases the order is the C3 linearisation, used only when every class of the hierarchy could be
     read. A name that no class read so far declares gets no origin once the search meets a base
     that could not be read, and none at the end (attributes added at run time, the metaclass's
     members after `Class.`). After a class with a decorator, the methods a decorator such as
     `@dataclass` may generate (`__init__`, `__repr__`, `__eq__`, the ordering methods,
     `__hash__`, `__setattr__`, `__delattr__`, `__replace__`, `__match_args__`) get no origin
     from the classes above it, unless the class writes them itself.
   - Only the server's own items get an origin; templates, keywords and the added standard
     library names keep theirs (the module, behaviour 40).
45. **v10 – Origin in the list, without slowing typing:** the list opens and narrows as before; the
   origin is asked for once per list, in the background, after the list is shown (one type
   definition, then one definition per base class), and the rows are redrawn when it arrives. A
   row shows it at the right edge after the type, separated by " · " (`int · Item`, `float ·
   Item` after the signature `()`, `str · object`), or alone when the row has no type. The right
   column is cut as before (behaviour 15). Signature and type keep coming from resolving the ten
   rows around the selection (behaviour 28); nothing is resolved for rows that are not shown, and
   no key waits for either answer. Not for files of a remote project. [GEN; OBS-0034 item 36]
46. **v10 – `complete_code`:** every item has a field `origin` when behaviour 44 gives one. For
   Python files `complete_code` now resolves the returned items without being asked, so `detail`
   (the declaration), `label_detail` (parameters) and `description` (type) are filled: items are
   resolved in the order returned until 500 ms are used up, the rest stay as the server sent
   them; the origin is added afterwards. `details: true` resolves all returned items without that
   limit, as before, for any language; `details: false` resolves nothing and adds no origin.
   Other languages are unchanged without `details`. [ADR-0008; OBS-0034 item 36]
47. **v11 – the list's keys from the keymap:** inserting the selected suggestion (behaviour 4, Enter)
   is the keymap action `EditorChooseLookupItem`, and inserting it over the rest of the identifier
   (v5 behaviour 14, Tab) is `EditorChooseLookupItemReplace`. While the list is shown and its
   editor has the focus, every key the keymap in use binds to one of them does that, with or
   without modifiers, before any other action on the same key; a key bound to both replaces.
   With the bundled keymaps these are Enter and Tab as before. Settings › Keymap lists both with
   the titles "Insert Suggestion" and "Replace with Suggestion" (group Completion List), and a
   key taken away there no longer acts on the list but does what it does in the editor. Up, Down
   and Escape stay fixed keys of the list. Run from Find Action or a menu while no list is
   shown, both actions answer that they are not available here. The hint line keeps its wording.
   [OSS (action ids and keys of the ported keymaps), GEN, SPEC-0028]
48. **v13 – Class name completion:** `ClassNameCompletion` (⌃⌥Space in both bundled keymaps)
    asks for the suggestions at the caret as `CodeCompletion` does (behaviour 1) and shows only
    those the server marks as a class, an interface, an enum or a struct, in the server's order,
    with the list's keys and filtering as before. Without such a suggestion the status bar says
    "No class names to suggest". In a SQL file and in a file without a language it does nothing
    but say so in the status bar, like Code Completion there. macOS may take ⌃⌥Space to select
    the next input source (docs/research/KEYMAP_COVERAGE.md); the action is in Code › Class Name
    Completion and in Find Action too. No MCP tool of its own: `complete_code` returns the
    kinds. [OSS: id, key and title of the open platform; GEN]
49. **v13 – Insert with a dot:** while the list is open, `EditorChooseLookupItemDot` (⌃. in both
    bundled keymaps) inserts the selected suggestion as Enter does (behaviour 47) and then types
    `.` behind it; when the suggestion ends in an empty pair of parentheses with the caret
    between them, the dot comes after the closing one. When the inserted suggestion opens
    places to fill in (behaviour 32's snippets), the dot is not typed and the caret stays at the
    first place. Without a list the key does nothing in the editor. [OSS: id and key of the
    bundled keymaps; GEN]
50. **v14 – Insert Live Template:** `InsertLiveTemplate` (⌘J in the bundled macOS keymap, Ctrl+J
    in the default one; Code › Insert Live Template…) asks for the suggestions at the caret as
    `CodeCompletion` does (behaviour 1) and shows only those the server marks as a template (kind
    `snippet`: rust-analyzer's `tfn`, `tmod`, jdtls's `sout`, `main`, …), in the server's order,
    with the list's keys and filtering; Enter inserts one with its places (behaviour 18). Without a
    template the status bar says "No live templates here", in a SQL file and in a file without a
    language too. Ferris IDEA has no template library of its own yet: the templates are the
    language server's (Out of scope). [OSS: id, key and title of the open platform; GEN]
51. **v14 – Next and Previous Template Variable:** the places of an inserted template (behaviour 18)
    are walked with the keys of `NextTemplateVariable` (Tab and Enter in the bundled keymaps) and
    `PreviousTemplateVariable` (⇧Tab) of the keymap in use, no longer with fixed keys. ⇧Tab
    selects the place visited before the current one with what was typed there, also from the last
    place; once Escape or leaving the text ended the template, ⇧Tab is the editor's again
    (unindent). (v19) At the first place ⇧Tab stays there: the place stays selected and the line
    is not unindented (before v19 the line was unindented, OBS-0086 K3). With the list of
    suggested names of a place open (behaviour 32), the keys of `NextTemplateVariable` that also
    choose from the list (Tab and Enter in the bundled keymaps) put the selected name into every
    place of the stop and go on to the next place in one press, as in the reference; before v19
    the first Tab only took the name and a second one was needed (OBS-0086 K2). ⇧Tab with that
    list open closes it and goes to the place before (or stays at the first). Escape with the
    list open closes only the list. Lists the server opens while typing in a place are not such
    a list: their keys stay those of behaviour 47. [OSS: ids and keys of the bundled keymaps; GEN;
    OBS-0086 for Tab; Enter by the same rule, not observed]
52. **v15 – Class names and ids in HTML:** in the value of a `class` or an `id` attribute of an
    HTML file the suggestions are Ferris IDEA's own (SPEC-0042 v22 behaviour 39). There the list
    opens while typing as in behaviour 8 although the value is a string, the typed name (behaviour
    2) reaches back to the start of the word in the value and includes `-` (`card-w`), and the list
    narrows over `-` as over letters (behaviour 3); it closes when white space or the closing quote
    is typed. The chosen name replaces the whole typed name. Elsewhere behaviours 2, 3 and 8 stay
    as they are. [OBS-0069 G1; STD: CSS class names hold `-`]
53. **v16 – Expand Live Template by Tab:** in a Python file, Tab with one caret and no
    selection, list or template expands Ferris IDEA's Python live template (behaviour 39) whose
    name is exactly the word before the caret, at the start of a statement, as if it were chosen
    in the list; a template whose name only starts with the word is not taken, and Tab then does
    what it did. Only where the keymap in use binds Tab to `ExpandLiveTemplateByTab`. [OSS, GEN]
54. **v17 – The best suggestions within the limit:** a server's list is put in the server's ranking
    — preselected first, then by sort text ignoring case first, then by the exact sort text, items
    without sort text last, the server's order where nothing decides (the order of behaviour 12)
    — before it is cut to the number of suggestions asked for: the window's limit and the `limit`
    of the MCP tool `complete_code`. `complete_code` returns its items in this order. Before v17
    the first items in the order sent were kept and returned as they came, so with rust-analyzer
    the type's own members came after methods of every type, and with intelephense a project
    function could be cut off behind a hundred built-in ones (OBS-0081). [STD, OBS-0081]

55. **v18 – `complete_code` gives the list people see:** the MCP tool asks the server as the
    editor does (behaviour 1, up to as many items as the editor takes) and narrows the answer by the
    typed prefix (behaviour 2: the identifier part left of the position, read from the file) with
    the same matcher function as the editor's list (behaviours 3, 12, 23 and 35: case-insensitive
    prefix matches first in the server's ranking, the exact name first, shorter first, then fuzzy
    matches; with nothing typed the whole list in the server's ranking). One code path serves both,
    so an agent and a person see the same suggestions in the same order. The answer is one page:
    `prefix`, `total` (matching items), `offset`, `items` (at most `limit`, default 50, at most
    500) and `next_offset` only when more follow; a further call with that `offset` gives the next
    page. Details and origins (behaviours 28 and 46) are resolved for the page only. Configuration
    keys (SPEC-0115 v7) and the class names and ids of an HTML value (SPEC-0042 v22) go through the
    same narrowing; in such a value the typed prefix is the typed name of behaviour 52, `-`
    included. The answer shape changes from a bare list to
    this object. [GEN; ADR-0008]
56. **v21 – The project's names first among equals:** each item whose name the project declares
    is marked with where: `file` when the edited file declares it, else `project` when another
    project file of the same extension declares it (at most 2000 files, read for at most 300 ms;
    each file's names kept by its modification time and size, so a second list reads only the
    changed files). Keywords, snippets and text items are never marked. The list and
    `complete_code` (`declared_in`) order the items first by the server's rank class, then the
    marked items before the others — the file's before the project's — then by the server's full
    ranking and the rules of behaviours 12 and 55. The rank class is the sort text without the
    item's label at its end, ignoring case: intelephense sends `0002` + the label for every name,
    which only orders by the alphabet, so its `greet` declared in the file came 15th behind the
    built-in `GRAPHEME_EXTR_*` constants and `grapheme_*` functions; now it comes first. Where a
    server ranks items apart (another class, preselect), its order stays: a built-in it ranks
    first stays first. Declared names come from the syntax tree (the tags query and the `name`
    of every declaring node), so the rule works for every language with a grammar. [F-038 PH1,
    GEN]
57. **v21 – Callables first in a call position:** when `(` follows the identifier at the caret
    (the rest of the identifier and blanks skipped, `echo gr|eet("you");`), items that are no
    function, method, constructor, class or struct come after those that are, before the
    server's rank class and the order of behaviour 56: a server may rank a constant ahead of the
    functions (intelephense on Linux ranked `GROUPING` second for `gr`). They stay in the list.
    [F-038 PH1, GEN]

## Acceptance criteria
- **AK-1** Completion and definition on unsaved text that differs from the file on disk (integration
  test with clangd).
- **AK-2** Prefix start, snippet text, prefix-then-fuzzy filtering (unit tests).
- **AK-3** In a real window (`ferris-idea ui --selftest`, with clangd): Ctrl+Space on `he|lper` lists
  `helper`; Escape closes without changes; Enter replaces the prefix.
- **AK-4** Use by hand: typing narrows, Tab, many suggestions, other languages.
- **AK-5** The macOS keymap maps ⌥Esc to `CodeCompletion` (unit test `completion_also_on_alt_escape_on_macos`); the self-test opens the list a second time with ⌥Esc on macOS.
- **AK-6 (v3)** When the list opens by itself, after identifiers and member access but not after numbers
  or operators (unit test `auto_trigger_after_identifiers_and_member_access`); in a real window typing the
  last letter of `helper` opens the list without Ctrl+Space (`--selftest`).
- **AK-7 (v4)** Labels and details with line breaks become one line (unit test
  `row_texts_are_single_line`); in a real window each shown detail lies inside its row (`--selftest`,
  painted bounds of the rows and their detail cells).
- **AK-8 (v5)** Kind icons, label pieces, prefix highlight positions and the right-hand type of a row
  (unit tests `row_pieces_from_label_details`, `prefix_positions_for_highlight`, `kind_icons`); label
  details, sort text, preselect and documentation are read from the server's items and announced in
  the client capabilities (unit test in `insight`).
- **AK-9 (v5)** Ranking: preselect, then sort text, then shorter first among prefix matches (unit test
  `server_ranking_before_length`).
- **AK-10 (v5)** Tab replaces the rest of the identifier, Enter does not (unit test
  `tab_replaces_the_identifier_rest`); in a real window the list shows the hint line (`--selftest`).
- **AK-12 (v6)** Row fitting: a row that fits keeps its texts; a long signature is cut with "…"
  and the type stays whole; with no room for the signature it is left out; only when label and type
  do not fit is the type cut (unit test `long_signature_gives_way_to_the_type`). Placement: the panel
  goes right when there is room, left when only the left has room, narrows on the wider side, is
  left out when no side has 160 px, and the list moves inside the window (unit test
  `documentation_panel_stays_in_the_window`). In a real window (`ferris-idea ui --selftest`) the
  painted completion list and documentation panel lie inside the window.
- **AK-13 (v7)** With jdtls (integration test `java_completion`): the varargs overload of `List.of` is
  a snippet `of(${0})`; `s.length().var` brings a postfix item whose range starts at `s`; `sout` is a
  template; `new ArrayLis` brings `ArrayList` with the import as additional edit; no `com.sun.` type
  is listed. Snippet, edit range and additional edits are read from items (unit test in `insight`).
- **AK-14 (v7)** Overload order and exact name first (unit tests
  `overloads_varargs_first_then_fewer_parameters`, `server_ranking_before_length`).
- **AK-15 (v7)** Snippet expansion: tab stops in visiting order, mirrored stops, indentation, choices,
  variables and escapes; the argument list at the end of a suggestion (unit tests
  `snippets_expand_with_stops_and_indentation`, `own_argument_list_at_the_end`).
- **AK-16 (v7)** In a real window (Java, TypeScript, Python), compared with the reference in OBS-0017:
  `List.o` starts with the varargs overload; Enter on a method inserts `()` with the caret inside;
  Tab over `isEmpty()` keeps the parentheses; `.var`, `.if`, `.for`, `.nn`, `.sout`, `sout`, `fori`,
  `psvm` expand with their tab stops; `ArrayList`, `bySku` and `restock` bring their imports.
- **AK-17 (v8, behaviours 25–26)** Expected type from declaration, assignment, argument and return;
  assignability (same type, type variables, widening and boxing, JDK table, the file's own supertypes,
  unions) (unit tests in `smart`); with jdtls (integration test `java_smart`): `List<String> l = |`
  lists `names` and `List.of`, not `s`; `names.add(|` lists `s`, not `names`; `return |` lists `s`.
- **AK-18 (v8, behaviour 27)** Garbled documentation is recognised, Javadoc is converted to Markdown,
  and without a source the text is cleaned up (unit tests in `javadoc`); with jdtls the documentation of
  `isBlank` has no stray `}` and no `jdt://` address (integration test `java_completion`).
- **AK-19 (v8, behaviours 28–30)** Signature and type from resolved details, call parentheses and their
  exceptions, postfix expression and templates (unit tests in `script_completion`); with
  typescript-language-server (integration test `ts_completion`): `item.` resolves `qty` to `number`, a
  function comes with `()`, `item.name.con` lists the `const` template over `item.name`.
- **AK-20 (v8, behaviours 31–33)** Template rows have the template icon, name placeholders are found,
  the type stop goes after the name stop (unit tests in `completion_model`); name suggestions from
  expression and type (unit tests in `names`).
- **AK-21 (v8)** MCP `smart_complete_code`, `suggest_names` and `complete_code` with `details` (protocol
  test listing the tools; headless `ferris-idea call` on a Java and a TypeScript project).
- **AK-22 (v9, behaviours 34, 42)** The first declaration of a pyright documentation on one line,
  its signature and type, the passed parameter count, overloads collapsed (unit tests
  `declaration_from_documentation`, `passed_parameter_count`,
  `documentation_with_overloads_collapsed`).
- **AK-23 (v9, behaviours 37–39)** Python call parentheses and their exceptions, postfix templates
  with expression and range, live templates at the start of a statement (unit tests
  `python_functions_get_parentheses`, `python_postfix_templates`, `python_live_templates`).
- **AK-24 (v9, behaviours 40–41)** Names and modules of stubs, the import's place, the names offered
  (unit tests `stub_names_and_modules`, `stdlib_import_placement`, `stdlib_names_offered`).
- **AK-25 (v9, behaviours 35, 37)** Sort text ignoring case and the typed case first; caret after a
  call without arguments (unit tests `sort_text_ignores_case_and_typed_case_first`,
  `caret_after_a_call_without_arguments`).
- **AK-26 (v9)** With pyright (integration test `py_completion`): `item.` lists `total_cents($0)`,
  resolves `qty` to `int` and `total_cents` to `()`/`int`, offers `if item:`; `Path` brings
  `pathlib.Path` with `from pathlib import Path` after `import os`; `iter` offers the template. In a
  real window, compared with the reference in OBS-0021: the items of OBS-0021 1–9 and 11, with
  `os.` typed right after opening (behaviour 36) and `item.t|qty` then Tab (behaviour 43).
- **AK-27 (v10, behaviour 44)** The receiver before the dot; what a class declares (methods,
  annotated and assigned names, `self` attributes, names in `if` blocks, nested classes); its
  bases with their places; the origin along a chain, with several bases in C3 order, behind an
  unreadable base, for generated methods of a decorated class and for a name nobody declares
  (unit tests `python_origin::tests`).
- **AK-28 (v10, behaviours 44, 46)** With pyright or basedpyright (integration test
  `crates/languages/tests/it/servers.rs::pyright_completion_names_the_origin`, skipped without one):
  after `item.` of a dataclass `Item` in another file `name` and `total` come from `Item`,
  `__class__` and `__sizeof__` from `object`, `__eq__` has no origin; after `s.` of a class with
  two bases the members come from the class, its two bases and `object`; after `e.` of a subclass
  of `ValueError` `args` comes from `BaseException`; a union and a module give none; resolving
  within a budget fills the items with their declaration, a generated `__eq__` gets none. `ferris-idea call <folder> complete_code`
  on a Python file returns `detail`, `description` and `origin` without `details`.
- **AK-29 (v10, behaviour 45)** The row's right column with type and origin, origin alone, and
  cut when too long (unit test `completion_model::tests::row_shows_the_origin_after_the_type`).
  In a real window, by hand: after `item.` the rows name `Item` and `object` a moment after the
  list opens, and typing on is not held up.
- **AK-30 (v11, behaviour 47)** The list's keys by the actions bound to the pressed key: Enter and
  Tab with the bundled macOS and default keymaps, a rebound key with a modifier, a key bound to
  neither, Up, Down and Escape, and nothing but closing while the focus is elsewhere (unit test
  `completion_model::tests::list_keys_follow_the_keymap`); the titles
  (`everywhere::tests::second_package_actions_have_titles`).
- **AK-35 (v12, behaviour 16)** The panel's place with a tool window left of the editor: right and
  narrowed where it went over the tool window before, left only inside the editor area, the roomier
  side with the right one first, none when no side has 160 px; inside the window, beside the list
  and never before the editor area for every place of the list (unit test
  `completion_model::tests::documentation_panel_prefers_the_right_and_stays_off_the_tool_window_on_the_left`).
- **AK-11 (v5)** Use by hand in a Java file: `List.o` shows icons, the highlighted `o`, signatures and
  right-aligned types, and the documentation of the selected overload beside the list.
- **AK-31 (v13, behaviour 48)** The kinds that stay and that go (`completion_model::tests::
  class_names_keep_types_only`); in a window of the test harness a list of mixed suggestions
  shown as class name completion holds only the types, and one without types says so
  (`eighth_package_keymap.rs::class_name_completion_keeps_types`). With a language server: not
  checked.
- **AK-32 (v13, behaviour 49)** ⌃. is the list's key (`completion_model::tests::
  list_keys_follow_the_keymap`); in a window, ⌃. on a suggestion inserts it with a dot, after the
  parentheses of an empty call (`eighth_package_keymap.rs::a_suggestion_is_inserted_with_a_dot`).
- **AK-33 (v14, behaviour 50)** Only templates stay (`completion_model::tests::
  live_templates_keep_snippets_only`); in a window of the test harness ⌘J in a text file says "No
  live templates here", and in a Rust file with a real rust-analyzer it lists `tfn` and no
  function, Enter inserts `#[test]` with the name selected
  (`tenth_package_keymap.rs::insert_live_template_lists_the_servers_templates`, skipped without
  rust-analyzer). With jdtls: not checked.
- **AK-34 (v14, behaviour 51)** Tab and ⇧Tab walk an inserted template's places both ways, ⇧Tab
  selects what was typed into a place and goes back from the end (v19: and stays at the first
  place); after Escape Tab is the editor's (`tenth_package_keymap.rs::tab_and_shift_tab_walk_a_templates_places`).
- **AK-36 (v15, behaviour 52)** `completion_model::tests::css_names_keep_their_hyphen`: the typed
  name of an HTML class value takes `-` along and white space ends it; in a window
  (`crates/workbench/tests/it/f038_window.rs::obs_0069_class_names_are_offered_in_html`): typing `s`
  in `class=""` opens the list by itself with `side`, typing `-` keeps a list of `side-bar`, Enter
  writes the whole name.
- **AK-37 (v16, behaviour 53)** Unit test
  `workbench_keymap_eleventh::tests::tab_expands_only_a_template_named_as_typed`; window test
  `eleventh_package_keymap.rs`: `tab_steps_out_of_pairs_and_expands_a_template` (macOS).
- **AK-38 (v17, behaviour 54)** An unsorted list is ranked before the limit: preselected items
  first, sort text ignoring case, items without sort text last; of 150 built-in items and one
  better-ranked project function with a limit of 100 the function is kept and first
  (`insight::tests::completion_is_ranked_before_the_limit`). Headless (OBS-0081): `complete_code`
  after `r.` in a Rust file lists `w`, `h`, `area()` before `into()`; in a PHP file `greet` is among
  the first 40 items for `gr`.

- **AK-39 (v18, behaviour 55)** The prefix left of a position and the narrowing of a server's
  list by it (`completion_filter::tests::{the_typed_prefix, a_server_list_is_narrowed_by_the_prefix}`:
  `<sec` keeps `section`, `sectionHeader`, then `select`; `the_typed_prefix_in_an_html_class_value`:
  `card-w` in a `class` value); the editor's list uses that function
  (`completion_model::filter` is it); through MCP, the configuration keys after `gre` come as
  `prefix` `gre`, `total` 2, and with `limit` 1 page by page with `next_offset`
  (`protocol.rs::configuration_keys_complete_and_quarkus_starts_in_dev_mode`). With a language
  server: an HTML file with `<sec` and a Rust file after `r.ar`, through `ferris-idea call` (agent
  run in PR).

- **AK-40 (v19, behaviour 51)** In a window of the test harness
  (`crates/workbench/tests/it/window_check_gaps_1118.rs::tab_on_a_places_name_list_goes_on`):
  a Java template `for (int ${1:i} = 0; ${1:i} < ${2:n}; ${1:i}++)` inserted with Enter selects
  `i` with the name list open; Tab takes the selected name and selects `n` with one press;
  ⇧Tab goes back to `i`, another ⇧Tab keeps `i` selected and the line's indentation unchanged;
  with the list open again, ⇧Tab at the first place keeps the place too. AK-34's test now expects
  ⇧Tab at the first place to stay.
- **AK-41 (v20, behaviour 13)** The documentation shown for the selected item is found again
  after the server added signature, type and documentation to it (behaviour 28): asked for
  before, answered before or after that, or brought along by the details answer
  (`completion_model::tests::the_first_items_documentation_survives_its_details`); an item
  without the server's own item keeps one entry per signature (overloads), and an item that has
  none shows no panel. In a window of the test harness with typescript-language-server
  (`crates/workbench/tests/completion_doc_first_item.rs::the_first_items_documentation_shows_at_once`,
  skipped without the server): after `xs.` on `const xs = [1, 2, 3]` Code Completion opens with
  `at` selected and its documentation beside the list without a key press; after typing `fil` the
  selected item shows its documentation too. Before v20 the documentation asked for at the
  opening was kept under a key made of label, signature and detail; when the details answer
  (behaviour 28) came first it changed that key and the answer was never shown.

- **AK-42 (v21, behaviours 56 and 57)** The rank class, the marks and the order with
  intelephense's sort texts, a call position and a server that ranks a built-in first
  (`completion_filter::tests::project_names_and_callables_first_among_ties`); the project's names
  from the other files of the same extension, read again after a change
  (`declared_names::tests::project_names_from_the_other_files`); declared names in Rust, Go and PHP
  (`declared_names::tests::declarations_in_three_languages`); with intelephense, `gr` in
  `echo gr|eet("you");` lists `greet` first, marked `file`, and the functions before the
  constants (`crates/languages/tests/it/side_by_side_f038.rs::intelephense_ranks_the_files_function_first`,
  skipped without intelephense). Not checked in a window.

## Out of scope
Parameter info (SPEC-0080), a live template library of Ferris IDEA's own with its settings page,
Surround with Live Template (⌥⌘J) and expanding a template's abbreviation by Tab (v14), Ferris IDEA's own Java live and postfix templates (Java names are the
server's: `sysout` and `nnull`, reached from `.sout` and `.nn` by behaviour 24), suggestions with
prefilled arguments (`toLowerCase(Locale.ROOT)`), sorting by usage, a scroll bar in the list,
smart type completion for Python (the reference has none either, OBS-0021 item 10); the origin of
members for other languages and other Python servers, for names that are not members (v10), and in
the window of a remote project.

## Known deviations from the reference
The reference ranks by its own analysis; Ferris IDEA follows the language server's ranking (behaviour 12), so
the order can differ where the server ranks differently. With the language server
pool (SPEC-0090) only the first list of a session waits for the server to start. First compared side by
side in OBS-0012 (2026-11-05, Java, `List.o`); the whole row (Java behaviour, TypeScript; Python only
in Ferris IDEA, the reference installation has no Python support) in OBS-0017.

Smart type completion (v8, behaviours 25–26) decides assignability from type names, not with a
compiler as the reference does. Subtypes outside the built-in JDK table and outside the open file are
not known (a project class `Dog` fits `Animal` only when both are declared in the open file); the type of
a longer expression (`names.get(0)`), constructors (`new ArrayList<>()`), narrowing after `instanceof`
and TypeScript generics beyond type variables are not looked at; TypeScript `return` has no expected
type (its document symbols carry no return type). The list can therefore miss a fitting suggestion or,
rarely, keep one that does not compile. Deviation: the hint line names the expected type the list is
filtered by (IMP-0302). TypeScript types (behaviour 28) appear a moment after the list, for the rows
shown. TypeScript calls (behaviour 29) put the caret inside the parentheses also for functions without
parameters, because the server gives the parameters only on resolve. The Java documentation repair
(behaviour 27) applies to the list, not to the documentation popup (SPEC-0077).

Python (v9, compared in OBS-0021): the right column shows the type, not the module that declares
the name as in the reference, whose column can name a module of another platform (`ntpath` for
`os.path.join` on a Mac; IMP-0315); importable names show their module in both. Ferris IDEA's postfix
`for` is offered for every expression, the reference's only for iterable ones. The live template
`main` writes `"__main__"` with double quotes. Standard library imports need the typeshed stubs of
pyright or basedpyright; with another Python server only the server's own imports are offered.

Python origin (v10, compared in OBS-0034 item 36): the reference names the origin of every member
from its own analysis; Ferris IDEA has only what pyright answers and what the sources declare, so
some rows stay without an origin (behaviour 44: receivers that are not a plain name, unions,
hierarchies it cannot read completely, generated methods of decorated classes, where the
reference shows `object`). The reference shows a method's parameters as `(self)`; Ferris IDEA shows
pyright's bound signature `()`. Ferris IDEA keeps the type in the right column and adds the origin
after it (IMP-0315). The origin arrives a moment after the list instead of with it.
- **Call position (v21):** the reference lists only functions in a call position; Ferris IDEA
  keeps the other names below the callables, so a constant or variable holding a closure can
  still be chosen.
- **Expand Live Template by Tab (v16):** only Ferris IDEA's own Python templates expand on Tab.
  The templates of other languages come from their language servers (behaviour 50), and Tab
  does not wait for a server's answer; there ⌘J lists them.
