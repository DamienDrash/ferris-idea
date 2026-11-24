# SPEC-0291: Parameter name hints

- **Version:** 9 · **Date:** 2026-11-17 (v9: a hint stands on a rounded pill of the scheme's
  hint background in every theme whose scheme gives one, behaviour 4b, AK-9, OBS-0079 L3);
  2026-11-13 (v8: Go, gopls is asked for parameter names, behaviour 3e; v2: no hints before the arguments of collection factory and
  varargs methods whose parameter names say nothing, behaviour 3a; v3: JavaScript and TypeScript:
  server settings, type hints, single arguments; OBS-0028 item 1; v4: the only argument of a
  constructor keeps its hint, OBS-0027 L18; v5: Kotlin, OBS-0035 L7; v6: no space between an opening
  bracket and a hint, asking a starting server again, OBS-0035 window check; v7: JavaScript and
  TypeScript: types behind class fields without an annotation, values behind enum members without an
  initializer, OBS-0033 L11)
- **Sources:** OBS-0011 step 2, finding 3 (in calls the reference shows the parameter names in grey
  before the arguments, but not before arguments that already say what they are; Ferris IDEA shows none),
  STD (LSP 3.17 `textDocument/inlayHint`: position, label as text or parts, kind `Parameter` = 2,
  `paddingLeft`/`paddingRight`), GEN (jdtls's `java.inlayHints.parameterNames.enabled` with the values
  `none`, `literals`, `all`; jdtls answers nothing for a range that reaches past the end of the file,
  seen while probing jdtls 1.64), OSS (scheme key `INLINE_PARAMETER_HINT`), SPEC-0058 (editor view),
  SPEC-0290 (requested like semantic tokens); v2: OBS-0018 finding 14 (Ferris IDEA shows `e1:` … before
  the arguments of `List.of(1, 2, 3)`, the reference shows none), GEN (the JDK's collection factories
  name their parameters `e1` … `e10`, `k1`/`v1` …, and `elements` for the varargs form); v7: OBS-0033 item 11 (L11: the reference shows type hints behind
  fields and the value behind enum constants, `= 0`; Ferris IDEA shows neither), GEN
  (typescript-language-server's preferences `includeInlayPropertyDeclarationTypeHints` and
  `includeInlayEnumMemberValueHints`, both off unless set; probed with typescript-language-server
  6.0.2: with both on, a field `count = 0` gets `: number` with kind `Type` and a space asked for
  before it, an enum member without an initializer gets `= 0` with no kind and a space asked for
  before it, a member with an initializer and a field with an annotation get nothing, and a `.js`
  class field gets its type the same way), STD (LSP 3.17: an inlay hint's kind is optional and has
  only the values `Type` and `Parameter`); v8: GEN (gopls's documented setting `hints` with the key
  `parameterNames`, off unless set, read from the section `gopls` of `workspace/configuration`),
  OSS (measured over the protocol with gopls 0.24.0: no hints without the setting; with it
  `scaled(all[0], 2.5)` gets `s:` and `factor:` of kind `Parameter`, and a variadic parameter is
  labelled `a...:`); v9: OBS-0079 L3 (in `Islands Light` the reference shows `sku:`, `name:`,
  `qty:` on a grey pill, Ferris IDEA as grey text without a background), OSS (the open scheme `Default`,
  parent of `Light`, gives `INLINE_PARAMETER_HINT` the background `EDEDED`,
  `Islands Dark` and `Dark` `393B40`; the theme palette's `inlay-bg` is used by no editor key)
- **Feature matrix rows:** F-013, F-038

## Purpose
See which parameter a literal argument fills without opening the method's declaration.

## Behaviour
1. **Request:** for a file whose server offers inlay hints (`inlayHintProvider`), the editor asks for
   the hints of the whole file (from the start to the end of its last line, never past it) when the
   file is opened and after the text the server has changed (SPEC-0290 behaviour 1). Ferris IDEA announces
   the client capability; jdtls is started with parameter names for literal arguments
   (`literals`). (v3) typescript-language-server gets its hint settings right after `initialized` with
   `workspace/didChangeConfiguration`, because it reads them only from there: parameter names for
   literals, types of variables and return values. [STD, GEN, OBS]
2. **Which hints:** only parameter names: hints of kind `Parameter`, or without a kind whose label ends
   with a colon (jdtls sends none). Type hints and others are not shown. A label made of parts is
   joined. [STD]
3. **Self-describing arguments:** a hint is left out when the argument already names the parameter —
   its last identifier, without a trailing `()` and a leading `get`/`is`, equals the parameter name
   ignoring case (`left` before `left`, `p.getLeft()` before `left`) — and when the argument is the
   only one of the call. [GEN]
2a. **(v3) Type hints:** in JavaScript and TypeScript hints of kind `Type` are shown too, without the
   filters of behaviours 3 and 3a; in other languages they stay off. [OBS]
2b. **(v7) Fields and enum members:** in JavaScript and TypeScript the server is also asked for the
   types of property declarations and the values of enum members, with the settings of behaviour 1:
   a class field without a type annotation shows its inferred type behind its name (`: number`
   behind `count` in `count = 0`), and an enum member without an initializer shows its value behind
   its name (`Red = 0`, `Green = 1`, and `C = 6` after `B = 5`). The field's type is a hint of kind `Type`
   (behaviour 2a). The member's value comes without a kind: in these two languages a hint without a
   kind whose label starts with `=` is a value hint; it is shown without the filters of behaviours 3
   and 3a and keeps the space the server asks for before it. A field with an annotation and a member
   with an initializer get no hint. No further hint kinds are turned on (parameter types, all
   parameter names, hints where the name matches the type stay off). [OBS-0033 L11, GEN, STD]
3b. **(v3) Single arguments:** in JavaScript and TypeScript the call's only argument keeps its hint
   (`console.log(message: "two")`); the rest of behaviour 3 applies. [OBS]
3c. **(v4) Constructors:** the only argument of `new Type(...)` keeps its hint (`new Dog(name: "Rex")`,
   also records, type arguments and qualified names); the rest of behaviour 3 applies. jdtls sends the
   hints; the two-argument case already worked. [OBS-0027 L18]
3d. **(v5) Kotlin:** kotlin-language-server 1.3.12 gives parameter hints only when its setting
   `kotlin.inlayHints.parameterHints` is on (tried: none without it, all with it sent in
   `workspace/didChangeConfiguration`); Ferris IDEA sends that after `initialized` with parameter hints on
   and type and chained hints off. The server then names every argument, so in `.kt` files a hint is
   kept only before an argument that starts with a literal — a string, a character, a number (also
   negative), `true`, `false`, `null` — and behaviours 3 and 3a apply. A call whose name starts with
   an upper-case letter is a constructor call, and its only argument keeps its hint as in 3c
   (`Circle(radius: 2.0)`), while `println("x")` shows none. [OBS-0035 item 11, STD]
3e. **(v8) Go:** gopls gives no hints unless asked; its question for the settings section `gopls`
   is answered with `hints.parameterNames: true`, no other hint kind. Behaviours 2, 3 and 3a apply
   as for every language; nothing is filtered for Go alone. [GEN, OSS]
3a. **(v2) Meaningless parameter names:** a hint is left out when its parameter name says nothing and
   the call is a collection factory or the parameter is a varargs parameter. A name says nothing when
   it is one letter followed by digits (`e1`, `e2`, `k1`, `v10`) or one of the generic names of a
   varargs parameter: `elements`, `items`, `values`, `objects`, `args`, `others`, `rest`. The call is a
   collection factory when the method's name (the identifier before the call's opening bracket, also
   after explicit type arguments like `List.<Integer>of`) is `of`, `copyOf`, `ofEntries`, `asList`, or
   ends in `Of` (`listOf`, `setOf`, `mapOf`, `arrayOf`); a label starting with `...` marks a varargs
   parameter. So `List.of(1, 2, 3)`, `Map.of("a", 1)` and `Arrays.asList(x, y)` show no hints, while
   `drawLine(0, 0, 10, 10)` with `x1`, `y1`, `x2`, `y2` keeps its hints (not a factory). [OBS, GEN]
4. **Look:** the hint is drawn in the text before its argument with the label (`name:`) in the
   `INLINE_PARAMETER_HINT` colours (foreground and background) and the padding the server asked for
   as a space. It takes room on the line: the text after it moves right, and carets, selections,
   underlines and clicks keep pointing at the right characters. The hint is not part of the text:
   copying, saving and the caret never see it. [GEN, OSS]
4b. **(v9) The pill:** the label stands on a pill in the background of `INLINE_PARAMETER_HINT`,
   one pixel wider than the label on each side, 2 px inside its line above and below, with 4 px
   corners, drawn under the text and over the caret row and selections. Every bundled scheme gives
   the key a background (`EDEDED` in `Light`, `Islands Light` and `Ferris IDEA Light`, `393B40` in
   `Islands Dark`, `Dark` and `Ferris IDEA Dark`), so every theme draws the pill; a scheme without one
   draws none. The space the server asks for stays outside the pill. Until v9 the background of
   behaviour 4 was set on the text run but never drawn: the editor paints its own backgrounds and
   the toolkit draws none for a run, so the hints showed as grey text in every theme (OBS-0079 L3).
   [OBS, OSS]
4a. **(v6) No space after the bracket:** the space a server asks for before a parameter name is left out
   when the hint stands at the start of its line, after white space or right after an opening bracket
   (`(`, `[`, `{`), so the call reads `Cat(name: "Tom")` and `total(a: 1, b: 2)` as with jdtls, which asks
   for no space before. kotlin-language-server asks for a space on both sides of every hint (recorded
   from a real answer), which gave `Cat( name: "Tom")`. After a comma without a space the space stays
   (`f(a: 1, b: 2)` for `f(1,2)`), and so does the padding of type hints (behaviour 2a). [OBS-0035 window
   check, STD]
1a. **(v6) Asking again:** a question that fails because the server is still starting is asked again as
   SPEC-0290 v3 behaviour 1a describes. [GEN]
5. **Edits:** until a newer answer arrives, hints after an edit move with the text and a hint at or
   inside the changed range is dropped; an answer for an older text is ignored. [GEN]
6. **Setting:** Editor › General › Code Insight › *Show parameter name hints* (on by default); off,
   the editor neither asks nor draws hints. Also `parameter_hints` in `set_ide_settings`. [GEN,
   ADR-0008]
7. **Agents:** MCP tool `inlay_hints` (path, optional first and last line): the parameter name hints
   as the editor shows them, with 0-based line, character and label. (v7) Each hint says what it is:
   `is_type` for a type hint, `is_value` for an enum member's value. [ADR-0008]

## Acceptance criteria
- **AK-1** Parsing: a recorded jdtls answer and an answer with label parts and kinds give the
  parameter hints at the right characters (UTF-16 positions); type hints are dropped; the request's
  range ends at the end of the last line (unit tests).
- **AK-2** Filtering: `add(left, 2)` hides `left:` and keeps `right:`; `p.getLeft()` and `isLeft()`
  hide `left:`; a single argument hides its hint, also across nested calls and strings with commas
  (unit tests).
- **AK-2a (v2)** Factories: `List.of(1, 2, 3)` with `e1`–`e3`, `Map.of("a", 1)` with `k1`/`v1`,
  `List.<Integer>of(1, 2)`, `Arrays.asList(1, 2)` with `elements`, `listOf(1, 2)` and a varargs label
  `...values:` hide the hint; `drawLine(0, 0, 1, 1)` with `x1` and a factory with a meaningful name
  (`Path.of("a", "b")` with `first:` and `more:`) keep it; the MCP/editor filter applies the rule
  (unit tests `inlay::tests::factory_arguments_hide_meaningless_hints`).
- **AK-2b (v3)** With typescript-language-server installed the hints of a `.ts` file hold parameter
  names before literals, the only argument's name and `: string` for an inferred variable (test
  `crates/languages/tests/it/servers.rs`, skipped without the server); `parse_kinds` keeps kind 1 only
  when asked (unit test).
- **AK-2e (v7)** OBS-0033 L11. The settings for typescript-language-server carry
  `includeInlayPropertyDeclarationTypeHints` and `includeInlayEnumMemberValueHints` for `typescript`
  and `javascript`, and none of the kinds that stay off
  (`servers::safe_mode_tests::typescript_hint_settings`). A recorded answer of the server gives
  `: number` as a type hint and `= 0`, `= 1` as value hints with the space before them; without the
  script flag (other languages) the value hints are dropped
  (`inlay::tests::field_types_and_enum_values`). With typescript-language-server installed a `.ts`
  file with `count = 0` in a class and `enum Color { Red, Green }` has `: number` behind `count`,
  `= 0` behind `Red` and `= 1` behind `Green`, nothing behind an annotated field or a member with an
  initializer, and a `.js` class field has its type
  (`crates/languages/tests/it/servers.rs::typescript_field_and_enum_hints`, skipped without the server).
  The window draws whatever `parameter_hints_in` returns, with the space before the label
  (`inlay_model::tests::a_value_hint_stands_behind_its_name`).
- **AK-2c (v4)** `constructor_argument` (`new`, qualified names, type arguments, not `renew(1)` or a nested
  call; unit test `inlay::tests::single_constructor_arguments_keep_their_hint`), and with jdtls `new Dog("Rex")` and
  a record's `new Circle(2.0)` have `name:` and `radius:` (`crates/languages/tests/it/java_generate.rs`).
- **AK-2d (v5)** Literal arguments and constructor calls by name
  (`inlay::tests::kotlin_literal_arguments`); the server's settings carry
  `kotlin.inlayHints.parameterHints` (`servers::safe_mode_tests::servers_in_safe_mode`); with a real
  kotlin-language-server `Cat("Tom", "Ann")` shows `name:` and `owner:`, `Shape.Circle(2.0)` shows
  `radius:`, and arguments that are variables or calls and the only argument of `println` show none
  (`crates/languages/tests/it/kotlin_structure.rs::kotlin_parameter_hints_with_the_real_server`,
  skipped without the server).
- **AK-3** Render model: a line with two hints has its display text with the labels inserted, every
  original column maps to the display offset after the hints before it, and a display offset maps back
  to its column; edits move or drop hints (unit tests).
- **AK-3a (v6)** An answer with a space asked for on both sides of every hint: the hints after `(`,
  after `, ` and at the start of a continuation line get none before them, the one after a comma without a
  space and the type hint keep it, all keep the one after
  (`inlay::tests::no_space_before_a_hint_after_the_bracket`). With the real kotlin-language-server none of
  the four hints of AK-2d has a space before it and each has one after
  (`kotlin_parameter_hints_with_the_real_server`). The retry rule is SPEC-0290/AK-8.
- **AK-4** In a real window (`ferris-idea ui --selftest`): in the Java insight file with jdtls installed,
  `add(1, count)` shows `left:` before `1` and no hint before `count`, and the text itself is
  unchanged.
- **AK-5** The MCP tool lists the hints of a C file when clangd is installed: `add(1, 2)` gives `a:`
  and `b:` before the literals (protocol test, skipped without clangd).

- **AK-9 (v9, behaviour 4b)** `editor_view::tests::a_hint_pill_wraps_its_label_inside_the_line`
  (size and corner of the pill); window test `crates/workbench/tests/islands_light_editor.rs`: the
  four hints of `Inventory.java`'s `main` line are drawn on four pills of the scheme's hint
  background in `Islands Light`. Side by side with the reference: open (coordinator's window check
  of OBS-0079).
- **AK-6 (v8)** `servers::tests::readiness_and_settings_of_clangd_intelephense_css_and_gopls` (the
  setting); with gopls (`crates/languages/tests/it/servers.rs::gopls_parameter_hints`):
  `scaled(1.5, 2.5)` gets `value:` and `factor:` before its arguments. Skipped without gopls.

## Out of scope
Type hints outside JavaScript and TypeScript, chained-call hints, hints in other places (lambda parameters, return values), clicking a
hint to go to the parameter, the per-method exclusion list, a smaller font for hints,
`workspace/inlayHint/refresh`, resolving hints.

## Known deviations from the reference
The reference decides itself which arguments get a hint; Ferris IDEA relies on what the server reports
and filters as in behaviours 3 and 3a, so the set of hints may differ per language. Hints are drawn with the
editor font, not a smaller one.

v5 (OBS-0035): the reference writes Kotlin hints as `name =`, the way a named argument is written;
Ferris IDEA keeps `name:` as in every other language.

v7 (OBS-0033 L11): field types and enum values are shown as typescript-language-server reports them;
with TypeScript 7's own server (`tsc --lsp`, SPEC-0042) no hint settings are sent, so these hints are
not expected there. Not compared in the window next to the reference yet.
