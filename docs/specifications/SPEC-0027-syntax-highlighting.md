# SPEC-0027: Syntax highlighting with Tree-sitter

- **Version:** 12 · **Date:** 2026-11-15 (v12: AK-15; v11: reassigned Java locals and parameters underlined, OBS-0050; v10: doc comments and Java fields by syntax, OBS-0047 T2/T3; v9: fields have their own colour in the brand schemes; v2: C++; v3: thirteen more languages, Markdown colour keys; v4:
  Kotlin; v5: Ruby and PHP; v6: colour roles per language after OBS-0033; v7: SQL built-in functions in
  the function-declaration colour, italic, after the AK-9 window check; v8: Kotlin functions and
  properties by role, OBS-0035 L8)
- **Sources:** GEN (editors colour code by syntax and keep colouring while typing; F-013), STD
  (Tree-sitter parsing, incremental re-parse with edits, highlight queries with capture names such as
  `keyword`, `string`, `comment`), ADR-0003/ARCHITECTURE (Tree-sitter for syntax), grammar crates with
  their bundled `highlights.scm` (all MIT); v6: OBS-0033 (side by side), OSS (`JetBrains/intellij-community`: the
  Python plugin's highlighter keys and their default fallbacks, ported per `docs/legal/APACHE_PORTING.md`);
  v10: OBS-0047 T2/T3, OSS (the bundled open scheme `IslandSchemeDark.xml`: `DEFAULT_DOC_COMMENT`,
  `DEFAULT_INSTANCE_FIELD`, `DEFAULT_STATIC_FIELD`), STD (Javadoc, KDoc, JSDoc, GroovyDoc, ScalaDoc,
  PHPDoc and rustdoc comment forms; Java scoping rules, JLS §6.4)
- **Feature matrix rows:** F-013

## Purpose
Turn source text into coloured spans quickly and keep them correct after every edit without re-parsing
the whole file.

## Behaviour
1. Languages: Python, JavaScript (with JSX), TypeScript, TSX, Java, Rust, Go, C, C++ (the C++ grammar with C's highlight query plus its own; v2), SQL, JSON, HTML, CSS. Folding regions and definitions follow (SPEC-0030, SPEC-0031).
   TypeScript uses the JavaScript queries plus its own; TSX adds the JSX queries. Other languages have no
   highlighting yet. [STD]
   **v3:** also XML, YAML, Markdown (block structure: headings, code fences), TOML, Shell (bash
   grammar), Dockerfile (containerfile grammar), Groovy (with IDECraft's own query, as the grammar ships
   none), Scala, SCSS, Less, Swift, Lua and R. Markdown captures use the reference's `MARKDOWN_*` colour
   keys with their fallbacks (headings → constant, code fences → string; logic template
   `MarkdownHighlighterColors`). [STD, OSS]
2. Spans are non-overlapping, sorted, in char offsets, each with a kind: the capture name of the
   grammar's highlight query (e.g. `keyword`, `function.method`, `string`). Where captures overlap, the
   innermost node wins; for the same node the earlier query pattern wins (Tree-sitter convention). [STD]
3. A document keeps its syntax tree. An edit (char range replaced by text) is applied to the tree and the
   text is re-parsed incrementally; the result names the char ranges whose syntax changed, so only those
   need repainting. Highlighting a range only queries that range. [STD]
4. After any sequence of edits, highlighting equals highlighting the final text from scratch. [GEN]
5. Parsing never fails: syntax errors give an error node and the rest is still highlighted. [STD]
6. MCP tool `highlight_file` (path, optional line range): tokens with 0-based line, start and end
   character and kind. [ADR-0008]

**v4:** Kotlin files are highlighted with the `tree-sitter-kotlin-ng` grammar (MIT) and an own query
(`queries/kotlin-highlights.scm`: keywords, comments, strings, numbers, types, annotations, function
names), since the grammar ships none. [GEN]

**v5:** Ruby files are highlighted with `tree-sitter-ruby` and PHP files with `tree-sitter-php` (its
PHP-with-HTML grammar, so text outside `<?php … ?>` is plain), each with the grammar's bundled
highlight query (both MIT). [STD]

**v6 – Colour roles per language (OBS-0033):** a capture gets the colour role the reference gives the same
token. Where a language has its own role, a language-specific capture (`<role>.<lang>`, e.g.
`attribute.python`) maps to its own key with a fallback to the neutral key, so a scheme without the key
still shows the neutral colour. Required roles:
- **Python:** decorators → metadata (`PY.DECORATOR` → `DEFAULT_METADATA`); `self`/`cls` as first parameter →
  self parameter (`PY.SELF_PARAMETER`, with the fallback the ported plugin defines); names in
  `def` → function declaration; a docstring (first string statement of a module, class or function) → doc
  comment (`PY.DOC_COMMENT` → `DEFAULT_DOC_COMMENT`). [OBS-0033 L1–L4, OSS]
- **TypeScript/JavaScript:** method and function declarations, including in interfaces → function
  declaration; `constructor` → keyword; method and function calls → plain text (the reference does not colour
  calls by default); the globals `Math`, `JSON`, `console`, `window`, `document`, `globalThis` → global variable
  (`JS.GLOBAL_VARIABLE`, italic); type parameters → type parameter; regular expressions → `JS.REGEXP`.
  [OBS-0033 L6–L10]
- **SQL:** column names in a column definition → column (`SQL_COLUMN` → `DEFAULT_INSTANCE_FIELD`); data
  types and `NULL` → keyword; built-in functions (`COUNT`, `SUM`, `MIN`, `MAX`, `AVG`, `COALESCE`, `NOW`,
  `CURRENT_DATE`, `CURRENT_TIMESTAMP`, …) → predefined symbol, italic; every keyword (also `DEFAULT`, `DESC`,
  `ASC`) → keyword; numbers → number; an alias where it is used → plain text. [OBS-0033 L12–L17]
- **SQL, v7:** built-in functions take IDECraft's own key `SQL_BUILTIN_FUNCTION`, always italic, whose
  colour falls back to the foreground of `DEFAULT_FUNCTION_DECLARATION`: a scheme without the key shows
  them in the function-declaration colour, italic (the reference showed them italic in the same blue as
  function declarations; no value taken from it). This replaces "predefined symbol" above. [OBS-0033 L14,
  window check of AK-9, decision of the coordinator under the project owner's standing approval]
- Ligatures stay as the font setting says; the default font setting turns them off, as in the reference
  (OBS-0033 L5). [OBS-0033]

**v8 – Kotlin roles (OBS-0035 L8):** the Kotlin query gives names the role the language server's tokens give
them too (SPEC-0290 v3), so a file looks the same before the server answers and without one, and has the
roles a Java file has:
- the name of a function where it is declared → function declaration (`function`);
- the name of a called function — `total(1, 2)`, the last name of `pet.describe()`, also a constructor call
  such as `Cat("Tom")` → function call (`function.call`);
- the name of a property where it is declared — a `val`/`var` of a file, a class body or an enum body, and a
  `val`/`var` parameter of a primary constructor → field (`property`);
- the name after the dot of a member access that is not called (`shape.radius`, `this.age`) → field, unless
  it starts with an upper-case letter (a type or a constant, as in `Shape.Circle`, `Math.PI`);
- parameters and local variables (a `val`/`var` in a function body, a lambda or an initializer) keep the
  text colour, where they are declared and where they are used.
Before v8 declared functions and unqualified calls shared one role (`function.method`) and member calls
and properties had none. Which colour a role has is the scheme's business: `Dark`, `Light` and the Islands
schemes give function declarations and fields their own colour and leave calls plain; the brand schemes
`IDECraft Dark` and `IDECraft Light` (SPEC-0060 v6) colour declarations and calls alike. Since v9 they give
fields the colour of numbers and constants (Aurora purple, lightened `C3A2BD` on dark, darkened `85597E` on
light), in every language: Java fields, Kotlin properties, record components, SQL columns. Before v9 a field
had the text colour there (decision of the Project Owner, 2026-11-11). [OBS-0035 item 13, GEN, STD]

**v10 – Doc comments and Java fields (OBS-0047 T2, T3):** the reference shows Javadoc green and italic
and Java fields purple; IDECraft showed both like plain comments and plain text. A file looks the same
before its language server answers and without one:
- **Doc comments** get the role `comment.documentation` (key `DEFAULT_DOC_COMMENT`, in `Islands Dark`
  `5F826B` italic): a block comment beginning with `/**` (not the empty `/**/`) in Java, Kotlin
  (KDoc), JavaScript and TypeScript (JSDoc), Groovy, Scala and PHP; in Rust also `///` (not `////`),
  `//!` and `/*! … */` — the Rust grammar's query named them but its plain-comment pattern came first
  and won. Python docstrings keep `PY.DOC_COMMENT` (v6). Other comments, and every comment of other
  languages (C, C++, Go, Swift, …), stay comments. Tags inside a doc comment (`@param`) are not set
  apart yet.
- **Java fields** get the role `property` (key `DEFAULT_INSTANCE_FIELD`, `C77DBB` in `Islands Dark`)
  and static ones `property.static` (key `DEFAULT_STATIC_FIELD`, `C77DBB` italic): the name of a
  field where it is declared (`static` in its modifiers → static), of an interface field and of an
  enum constant (both static), of a record component; the name after the dot of a field access
  (`this.count`, `p.x`; a name all in capitals such as `Color.RED` → static; any other name with a
  capital first letter is taken for a type and keeps its colour); and a plain name inside a class
  that one of the enclosing class, enum, interface or record bodies declares as a field, unless a
  parameter of an enclosing method, constructor or lambda, a local variable of an enclosing block,
  `for` header, `switch` group or `try` resource, a loop variable or a `catch` parameter of the same
  name hides it (a local hides the field in its whole block, also before its declaration). Method
  names, types, labels, package and import names, parameters and locals keep their colour.
  Inherited fields and static fields of other classes used by name are not known from the file and
  keep the text colour; the server's tokens (SPEC-0290) colour them where a Java server runs.
- **Servers:** where a server sends semantic tokens, they still win over these roles (SPEC-0290
  behaviour 3): jdtls and kotlin-language-server send `property` (with `static`/`readonly`),
  typescript-language-server `property`, rust-analyzer `property` for struct fields — all of which
  map to the field keys already. A comment is no semantic token, so doc comments always keep their
  syntax role.
- **Brand schemes:** `IDECraft Dark` and `IDECraft Light` define `DEFAULT_STATIC_FIELD` as their field
  colour in italics (SPEC-0060 v11), and their doc comments stay their comment grey in italics, as
  their files say. [OBS-0047, OSS, STD, GEN]

**v11 – Reassigned Java locals and parameters (OBS-0050):** the reference underlines a local variable that
gets a new value after its declaration (`i` of `for (int i = 0; i < 2; i++)`) at every place it is named —
its declaration and each use — with a thin line in a muted colour, while the text keeps its colour; fields,
and locals and parameters that never change, are not underlined. The underline is no problem: it is not
listed and not counted. IDECraft does the same by syntax, without a language server: [OBS-0050, STD, OSS]
- **Reassigned:** a parameter of a method, constructor or lambda that is the target of an assignment (`=`,
  `+=`, …) or of `++`/`--` anywhere in its scope; a local variable that has a value from the start — declared
  with one, the variable of an enhanced `for`, a `catch` parameter — and is such a target; a local declared without a value that is the target
  of `++`/`--` or of a compound assignment, or of a plain assignment inside a loop that does not also hold
  its declaration, or of two plain assignments that are not in different branches of the same `if`/`else`,
  `switch` or `? :` (one in each branch is a first assignment on every path, as `final` allows).
- **Where:** the name in the declaration and every name that refers to that variable in its scope (the
  innermost declaration of that name in an enclosing method, constructor, lambda, block, loop, `catch` or
  `switch` group, declared before the use). A class body inside the method (an anonymous or local class)
  starts afresh: the outer locals it can see are effectively final and never reassigned, and its own
  members are analysed on their own. `this.x` and `obj.x` are fields, not locals.
- **Colour:** the roles `variable.reassigned` (key `DEFAULT_REASSIGNED_LOCAL_VARIABLE`) and
  `variable.parameter.reassigned` (key `DEFAULT_REASSIGNED_PARAMETER`); in `Islands Dark` both have the text
  colour with an underline in `84868C`. A scheme's `EFFECT_TYPE` 1 (a line under the text) with an
  `EFFECT_COLOR` draws that line under a syntax run in the editor; a scheme without the keys falls back to
  the local or parameter key (no underline).
- **Servers:** the underline is drawn over the server's semantic tokens (SPEC-0290), which do not say
  whether a variable is reassigned.

## Acceptance criteria
- **AK-1** A Rust snippet gives `keyword` for `fn`/`let`, `function` for the defined name, `string` for the
  literal and `comment` for the comment, at the right char offsets (with non-ASCII text before them).
- **AK-2** Every supported language highlights a small sample with at least one keyword or string span and
  without overlaps; an unsupported language yields an error naming it.
- **AK-3** Incremental edits report changed ranges covering the edit, and after a series of random edits
  the spans equal a fresh highlight.
- **AK-4** Broken code still yields spans after the error.
- **AK-6 (v4)** The Kotlin sample highlights (part of AK-2's test).
- **AK-7 (v5)** The Ruby and PHP samples highlight (part of AK-2's test).
- **AK-8 (v6)** For each sample of OBS-0033 (Python, TypeScript, SQL) the token named in each role of
  behaviour v6 resolves, in the `Dark` scheme, to the key and colour named there (unit tests over
  `highlight` plus `capture_key`/scheme lookup; one test per language). v7: SQL built-ins are tested also
  through the editor's per-line styling (`syntax_runs`): colour = `DEFAULT_FUNCTION_DECLARATION`'s
  foreground in `Dark`, italic.
- **AK-9 (v6)** In a real window with the `Dark` scheme the three sample files look like the reference's in
  the roles above (window check next to the reference).
- **AK-5** The MCP tool returns tokens for a workspace file, limited to the requested lines.
- **AK-10 (v8)** A Kotlin sample with a class, constructor parameters with and without `val`, properties, an
  initializer, a function with locals, a lambda, member calls, member access and an extension function gives
  exactly the roles of behaviour v8, and nothing for parameters and locals (unit test
  `syntax::tests::kotlin_colour_roles`). In every bundled scheme the roles `function`, `function.call` and
  `property` resolve to the style of the Java keys the server's tokens use, and in `Dark` function
  declarations and fields differ from the text colour (`theme::tests::kotlin_tokens_take_the_java_roles`).
- **AK-11 (v9)** In `IDECraft Dark` and `IDECraft Light` a field (`DEFAULT_INSTANCE_FIELD`, and the role
  `property`) has the colour of numbers and constants and differs from the text colour (unit test
  `theme::tests::brand_schemes_give_fields_the_constant_colour`).

- **AK-12 (v10)** A Java sample with instance and static fields, a parameter and a local hiding a field,
  a loop variable, an enum, an interface field, a record and field accesses gives exactly the field
  roles of behaviour v10, and parameters, locals, methods and loop variables keep theirs (unit test
  `syntax::tests::java_field_roles`).
- **AK-13 (v10)** `/** … */` is a doc comment in the seven languages, the four Rust forms are, `/**/`,
  `////` and other comments are not, and C keeps plain comments (unit test
  `syntax::tests::doc_comments_have_their_own_role`); the roles take the scheme's keys and styles in
  every bundled scheme (`theme::tests::doc_comments_and_fields_take_the_scheme_colours`).

- **AK-14 (v11)** A Java sample with a loop counter, a parameter that gets a new value, a local with a
  value assigned again, a local assigned once in each branch of an `if`/`else` (quiet), a local assigned in
  a loop, a compound assignment, an anonymous class with a field of the same name as an outer local
  (quiet), and fields changed with `++` (quiet) gives the reassigned roles at exactly the declarations and
  uses (unit test `syntax::tests::java_reassigned_roles`); in `Islands Dark` the local and the parameter
  role have the text colour, `underline` and the effect colour `84868C`
  (`theme::tests::reassigned_keys_underline`); the editor's run for such a name carries the underline
  (`editor_view::colour_role_tests::reassigned_locals_are_underlined`).

- **AK-15 (v12)** Window test (`crates/workbench/tests/it/f038_window.rs`:
  `hand_test_110_ruby_and_php_are_coloured`): in an opened Ruby and PHP file the keyword, the
  comment, the string and (PHP) the variable carry their captures and scheme colours, and keyword,
  comment and string differ in colour (`EditorView::syntax_style_at`).

## Out of scope
Themes and colours (UI, ADR-0009), language injections (e.g. CSS inside HTML, Markdown inline
emphasis), semantic highlighting
from language servers, folding (own spec).

## Known deviations from the reference
The reference uses its own lexers; colours come with the theme port (M3.8). Token boundaries may differ
per language. v6: Kotlin files without a Kotlin module keep full highlighting (the reference turns analysis
off there, OBS-0033 item 13).

v8 (OBS-0035): the reference sets calls of top-level functions in italics and marks a variable that a type
check has narrowed (smart cast); IDECraft does neither — the syntax tree does not know where a called
function is declared, and kotlin-language-server's tokens do not say it either. Until v9 a field had the
text colour in the default scheme `IDECraft Dark`, so Kotlin properties stood out only in the other schemes;
v9 gives fields their own colour there.
