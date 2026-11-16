# SPEC-0140: Kotlin inspections with types

- **Version:** 2 · **Date:** 2026-11-11 (v2: unused private declarations and parameters, OBS-0035 L3)
- **Sources:** GEN (IDEs warn about Kotlin mistakes that need types — results of String or
  BigDecimal calls dropped, `===` on values, arrays compared with `==` or printed as references,
  `equals` between unrelated types, integer division converted afterwards — and fix them from
  ⌥↩; F-037), STD (Kotlin language: `==` is `equals`, `===` is identity; arrays have reference
  `equals`/`toString` and `contentEquals`/`contentToString`/`contentDeepToString`; `vararg x: Int`
  is an `IntArray`, `vararg x: T` an `Array<T>`; the last expression of an `if` branch or lambda
  is its value; `println(CharArray)` prints the characters), SPEC-0139 (Java with types),
  SPEC-0098 (own inspections), ADR-0008
- **Feature matrix rows:** F-037

## Purpose
Kotlin files get the type-aware warnings Java got in SPEC-0139, with the same rule: report only
where the types are known from the file.

## Behaviour
1. **Type model** (per file, on the syntax tree): literals (`L` long, `f` float, unsigned unknown);
   parameters and constructor parameters with their written type (`vararg` as an array);
   properties with a written type or their initializer's type, locals only before the use;
   `this.x`; `.length` and `.size`; `arrayOf`/`intArrayOf`/… and `IntArray(n)`, constructor calls
   of `BigDecimal`, `BigInteger`, `StringBuilder`, `String`; well-known methods of String, numbers
   (`toInt`, `toDouble`, …), `BigDecimal`, `BigInteger`; `+` with a String, numeric operations,
   comparisons. Nullable types count as their base type. Anything else is unknown. [STD]
2. **Rules** (warnings): [GEN, STD]
   - `kotlin.ignored-result`: a call of a value-returning String/BigDecimal/BigInteger method whose
     result is dropped — not the last statement of an `if`/`when` branch or lambda (its value).
   - `kotlin.identity-equality`: `===`/`!==` between two Strings, two numbers, or two Chars or
     Booleans.
   - `kotlin.array-equality`: `==`/`!=` or `equals` between two arrays.
   - `kotlin.array-to-string`: `arr.toString()`, `println(arr)`/`print(arr)` (not `CharArray`),
     `"$arr"`, `"${arr}"`, `"…" + arr`.
   - `kotlin.equals-incompatible`: `a.equals(b)` between two different types of String, the
     number types, Char, Boolean, BigDecimal, BigInteger, StringBuilder.
   - `kotlin.integer-division`: `(a / b).toDouble()` or `.toFloat()` with integral `a`, `b`.
3. **Fixes:** "Assign the result to 'x'" when `x` is a `var`; `==`/`!=` for identity;
   `a.contentEquals(b)` (negated for `!=`); `arr.contentToString()` (`contentDeepToString()` for
   arrays of arrays; `${…}` in templates); `a.toDouble() / b`. Applied like SPEC-0125 (⌥↩, MCP
   `fix_inspection`).

4. **Unused private declarations (v2):** `kotlin.unused-private`, a weak warning drawn in the colour
   of unused code (SPEC-0125 v2 behaviour 6), on the name of a `private` class, object, function or
   property (also a `private val`/`var` of a primary constructor) whose name is written nowhere else
   in the file — as an identifier or as `$name` in a string template. A private declaration cannot
   be reached from another file, so the file decides. Matching is by name: any other mention of the
   name counts as a use, also of a different declaration with that name. Never reported: annotated
   declarations and everything inside an annotated class (frameworks reach those by reflection),
   `override` and `operator` members, `main`, the methods Java serialization calls by name
   (`readObject`, `writeObject`, `readResolve`, `writeReplace`, `readObjectNoData`), constructor
   properties of a `data` class, and anything that is not private — public, internal and protected
   declarations may be used from elsewhere. Local declarations are left to the compiler's own
   warning, which the language server reports. [OBS-0035 item 6, GEN, STD]
5. **Unused parameters (v2):** `kotlin.unused-parameter`, a weak warning in the same colour, on a
   parameter of a `private` function with a body when the function never mentions the parameter's
   name. Not for parameters starting with `_`, for functions that are annotated, `override`,
   `operator`, `open`, `abstract`, `external`, `expect` or `actual`, for functions in an interface
   or an annotated class, or for a function the file passes as a reference (`::name`), whose
   parameters its user fixes. No quick fix: removing a declaration or a parameter belongs to Safe
   Delete and Change Signature. [OBS-0035 item 6, GEN]

## Acceptance criteria
- **AK-1** Every rule reports its case and stays quiet on the near miss (parameters' `val` for the
  fix, `CharArray`, lambda and `if` values, unknown types); every fix gives the expected text
  (unit test `kotlin_inspections::tests::rules_with_types`).
- **AK-2** No finding in 1 496 Kotlin files of real Android projects on this Mac (scanned once,
  build output and worktree copies skipped; 2026-11-02).
- **AK-3** MCP: a Kotlin identity comparison with its fix (`crates/mcp/tests/it/protocol.rs`:
  `inspect_code_tool`).
- **AK-4** Window self-test (`java_inspection_steps`): `a === b` on Strings appears as an
  "IDECraft: …" problem; ⌥↩ offers "Replace with '=='" first and Enter applies it.
- **AK-5** Hand test (round 4): a Kotlin project next to the reference.

- **AK-6 (v2)** In one file: an unused private property, function, class and constructor property and
  the unused parameters of private functions are reported as weak warnings that count as unused
  code, with their messages; used ones (also through `$name` in a template), `const`, and a
  parameter starting with `_` are not. Nothing is reported for public declarations and their
  parameters, `main`, annotated declarations, members of an annotated class, constructor
  properties of a data class, `override`, `operator`, the serialization methods, and a function
  passed as `::name` (unit tests
  `kotlin_inspections::tests::unused_private_declarations_and_parameters`,
  `unused_stays_quiet_for_public_api_and_special_declarations`).

## Out of scope
Smart casts, nullability and data flow, generics, extension functions and types from other files
or libraries, `when` subjects, Kotlin compiler warnings (the language server reports those).

## Known deviations from the reference
The reference resolves all types and finds more; IDECraft reports only where the file shows the
types. Not compared side by side.

v2 (OBS-0035): the reference also greys public declarations that nothing in the project uses
(`owner`, `friendly`, `Person` in the observed file). IDECraft reports only what the file alone can
decide; unused public Kotlin declarations need a project-wide index of Kotlin uses (SPEC-0316 indexes
Java declarations only) and stay out. Because matching is by name, an unused private declaration that
shares its name with something else in the file is missed.
