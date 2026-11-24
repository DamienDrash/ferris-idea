# SPEC-0290: Semantic highlighting from the language server

- **Version:** 3 · **Date:** 2026-11-11 (v2: JavaScript/TypeScript keys after the window check of
  SPEC-0027 v6; v3: Kotlin keys and corrections, asking a starting server again, OBS-0035 L8)
- **Sources:** OBS-0011 step 2, finding 3 (in an opened Java file the reference colours method names in
  declarations, gives fields and record components their own colour and sets static calls and static
  fields in italics; Ferris IDEA colours by syntax only), STD (LSP 3.17 `textDocument/semanticTokens/full`:
  legend of token types and modifiers, relative five-number encoding, standard type names such as
  `method`, `property`, `parameter`, and modifiers such as `declaration`, `static`, `readonly`), OSS
  (colour-scheme keys of the open Java highlighter `JavaHighlightingColors` with their fallbacks to the
  language-neutral `DEFAULT_*` keys; logic template), SPEC-0027 (syntax highlighting), SPEC-0060 (colour
  schemes), SPEC-0090 (language server pool); v2: SPEC-0027 v6 (colour roles per language, OBS-0033
  L8/L9) and the window check of its AK-9 (server tokens painted global objects and type parameters
  over their syntax roles; decision of the coordinator under the project owner's standing approval, 2026-11-10)
- **Feature matrix rows:** F-013

## Purpose
Colour code by what a name means, not only by how it is written: a method being declared, a field, a
static member. The language server knows this; the syntax tree does not.

## Behaviour
1. **Request:** for a file whose language server offers full semantic tokens
   (`semanticTokensProvider.full`), the editor asks for them when the file is opened and again after
   the text the server has changed (the pool's sync, SPEC-0090). Ferris IDEA announces the client
   capability with the relative format. Servers without the capability, and files without a server,
   keep plain syntax colours. [STD]
1a. **Asking again (v3):** a question the server does not answer in time (30 s), answers with an error, or
   that ends because the server stopped, is asked again for the same text after 3, 10, 30, 60 and 120
   seconds — five times at most, then no more until the text changes. A server that is still importing a
   build (kotlin-language-server resolving a Gradle classpath, jdtls importing Maven) fails like that,
   and before v3 the file then kept its syntax colours until the next edit. A missing server and a server
   that cannot be started are not asked again; an answer ends the count; a changed text is asked about
   anyway (behaviour 1), and a closed tab or the switched-off setting ends the tries. The same holds for
   parameter name hints (SPEC-0291 v6). [GEN, found while probing kotlin-language-server 1.3.12:
   `initialize` took 15 s on an idle machine in the Gradle project of OBS-0035]
2. **Decoding:** the answer's numbers are read in groups of five (line delta, start delta, length,
   type index, modifier bits) against the legend the server announced; positions and lengths are
   converted from the server's position encoding to characters. Unknown type indexes are skipped. [STD]
3. **Colour keys:** each token maps to a colour-scheme key from its type and modifiers; the first row
   that matches wins. In Java files the Java key is used, which falls back to the neutral key in
   schemes that do not define it; other languages use the neutral key directly. [STD, OSS]

   | Token | Java key | Neutral key |
   |---|---|---|
   | `method`/`function` with `declaration` | `METHOD_DECLARATION_ATTRIBUTES` | `DEFAULT_FUNCTION_DECLARATION` |
   | `method`/`function` with `static` | `STATIC_METHOD_ATTRIBUTES` | `DEFAULT_STATIC_METHOD` |
   | `method`/`function` | `METHOD_CALL_ATTRIBUTES` | `DEFAULT_FUNCTION_CALL` |
   | `property`/`recordComponent` with `static` and `readonly`, `enumMember` | `STATIC_FINAL_FIELD_ATTRIBUTES` | `DEFAULT_STATIC_FIELD` |
   | `property` with `static` | `STATIC_FIELD_ATTRIBUTES` | `DEFAULT_STATIC_FIELD` |
   | `property`/`recordComponent` | `INSTANCE_FIELD_ATTRIBUTES` | `DEFAULT_INSTANCE_FIELD` |
   | `parameter` | `PARAMETER_ATTRIBUTES` | `DEFAULT_PARAMETER` |
   | `variable` with `static` | `STATIC_FIELD_ATTRIBUTES` | `DEFAULT_GLOBAL_VARIABLE` |
   | `variable` | `LOCAL_VARIABLE_ATTRIBUTES` | `DEFAULT_LOCAL_VARIABLE` |
   | `interface` | `INTERFACE_NAME_ATTRIBUTES` | `DEFAULT_INTERFACE_NAME` |
   | `class`/`struct`/`record`/`enum`/`type` | `CLASS_NAME_ATTRIBUTES` | `DEFAULT_CLASS_NAME` |
   | `typeParameter` | `TYPE_PARAMETER_NAME_ATTRIBUTES` | `DEFAULT_PARAMETER` |
   | `decorator`/`annotation` | `ANNOTATION_NAME_ATTRIBUTES` | `DEFAULT_METADATA` |

   Other types (`namespace`, `keyword`, `modifier`, `comment`, `string`, `number`, `operator`, …) get
   no key and keep their syntax colour.

   **v2 – JavaScript and TypeScript** (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`): the
   server never downgrades a role SPEC-0027 v6 gives these files. Before the table:

   | Token | Key |
   |---|---|
   | `variable`/`property` with `defaultLibrary` (e.g. `Math`, `JSON`, `console`) | `JS.GLOBAL_VARIABLE` (falls back to `DEFAULT_GLOBAL_VARIABLE`) |
   | `typeParameter` | `TYPE_PARAMETER_NAME_ATTRIBUTES` (the key of the syntax role, falls back to `DEFAULT_PARAMETER`) |

   All other tokens of these files map by the table's neutral column. [STD, SPEC-0027]

   **v3 – Kotlin** (`.kt`, `.kts`): the table's Java column, so a Kotlin function, property, parameter,
   local variable and type has the key — and with it the colour and style — of its Java counterpart, also
   in a scheme that sets the Java keys apart from the neutral ones. kotlin-language-server 1.3.12 sends
   the types `function`, `property`, `parameter`, `variable`, `class`, `interface`, `enum`, `enumMember`,
   `type` with the modifiers `declaration`, `abstract`, `readonly` (recorded from a real answer), all of
   which the table knows; the mapping itself was never the reason for a plain Kotlin file. [STD,
   OBS-0035 item 13]
3a. **Kotlin corrections (v3):** three things the server reports differently from what the code says are
   put right with the file's syntax tree (SPEC-0027) before the keys are chosen: [STD, GEN]
   - a `val`/`var` in a function body, a lambda or an initializer arrives as a `property` being declared;
     it is a `variable` (its uses already arrive as `variable`);
   - a `val`/`var` parameter of a primary constructor arrives as a `parameter` being declared; it is a
     `property` (its uses already arrive as `property`);
   - a `$name` in a string template arrives as one token over `$name`, followed by the token of `name`;
     the first is dropped, so the name keeps its own role.
   The MCP tool and a remote project's host report the corrected types.
4. **Layering:** a token's attributes (foreground, background, bold, italic) replace the syntax colour
   inside the token's range; outside tokens the syntax colours stay. [GEN]
5. **Edits:** until a newer answer arrives, tokens after an edit move with the text, tokens the edit
   touches are dropped, and an answer for an older text than the editor's is ignored. Typing never waits
   for the server. [GEN]
6. **Setting:** Editor › General › *Semantic highlighting from the language server* (on by default);
   off, the editor neither asks nor paints tokens. Also `semantic_highlighting` in `set_ide_settings`.
   [GEN, ADR-0008]
7. **Agents:** MCP tool `semantic_tokens` (path, optional first and last line): tokens with 0-based line,
   start and end character, type, modifiers and colour key. [ADR-0008]

## Acceptance criteria
- **AK-1** Decoding: a recorded jdtls answer for a small Java file yields the method declaration, the
  record components, the static call and the parameter at the right lines and characters, also after
  non-ASCII text and with UTF-16 positions (unit tests).
- **AK-2** Mapping: every row of the table maps to its key; Java keys fall back to the neutral keys in
  the bundled schemes, and a static field and a static method come out italic in Islands Dark (unit
  tests in `ferris-idea-ui`).
- **AK-3** Layering and edits: tokens override syntax runs only inside their range; an insert before a
  token moves it, an edit inside drops it; a stale answer is ignored (unit tests of the render model).
- **AK-4** In a real window (`ferris-idea ui --selftest`): opening a Java file with jdtls installed paints
  the declared method name in the method-declaration colour and the static field in italics.
- **AK-5** The MCP tool returns tokens with keys for a C file when clangd is installed, limited to the
  requested lines (protocol test, skipped without clangd).
- **AK-6 (v2)** In JavaScript and TypeScript files a `variable` or `property` token with `defaultLibrary`
  maps to `JS.GLOBAL_VARIABLE` and a `typeParameter` token to `TYPE_PARAMETER_NAME_ATTRIBUTES`; in `Dark`
  they resolve to the same style as the syntax roles `variable.global.js` and `type.parameter`, while
  other languages keep the v1 mapping (unit tests in `ferris-idea-ui`). Checked in the window with the
  TypeScript server running as part of SPEC-0027/AK-9.
- **AK-7 (v3)** Kotlin: `.kt` and `.kts` files take the Java keys for every token type the server sends
  (`theme::tests::kotlin_tokens_take_the_java_roles`); an answer in the server's legend and shapes decodes
  to the declared function, the constructor property as a field, the local `val` as a variable and one
  token for the name in `$name`, while the same answer for another language is left as sent
  (`semantic::tests::kotlin_tokens_take_java_keys_and_the_trees_roles`); the tree tells a local from a
  property and a `val` parameter from a plain one (`syntax::tests::kotlin_colour_roles`). With the real
  server over the files of OBS-0035: a declared function, a call through a receiver, a property read
  through a receiver, the constructor's `val` as a field, a local `val` as a variable and no token on a `$`
  (`crates/languages/tests/it/kotlin_structure.rs::kotlin_semantic_tokens_with_the_real_server`, skipped
  without kotlin-language-server).
- **AK-8 (v3)** Asking again: a timeout, a server error and a stopped server are asked again, a missing
  server, a failed start and a caller that gave up are not; the pauses are 3, 10, 30, 60 and 120 s and then
  end; an answer starts the count anew, per file (`semantic::tests::failed_requests_are_asked_again_a_few_times`,
  `semantic_model::tests::a_starting_server_is_asked_again`). Not checked in a window: a cold
  kotlin-language-server that needs longer than 30 s.

## Out of scope
Delta and range requests (`full/delta`, `range`), `workspace/semanticTokens/refresh`, rainbow
colouring of local variables, strike-through for `deprecated`, the per-language colour settings page.

## Known deviations from the reference
The reference computes meaning itself; Ferris IDEA relies on the language server, so colouring arrives a
moment after opening a file and depends on what each server reports. Not compared side by side yet
after the change.

v3 (OBS-0035 L8): kotlin-language-server reports a use of a parameter as a `variable` and says neither
that a called function is declared at the top level nor that a type check narrowed a variable, so
parameters used in a body have the local-variable key, top-level calls are not in italics and smart casts
are not marked, where the reference does all three.
