# SPEC-0139: Java inspections with types

- **Version:** 1 · **Date:** 2026-11-02
- **Sources:** GEN (IDEs warn about mistakes that need the types of the operands — Strings and
  boxed values compared with `==`, results of String/BigDecimal methods ignored, `equals` between
  unrelated types, arrays printed or compared as references, integer division assigned to a
  floating-point variable — and fix them from ⌥↩; F-037, F-032), STD (Java Language
  Specification: reference equality of objects, binary numeric promotion, integer division,
  `var`; `java.util.Objects.equals`, `java.util.Arrays.toString/deepToString/equals`;
  `PrintStream.println(char[])` printing the characters), SPEC-0125 (Java inspections), ADR-0008
- **Feature matrix rows:** F-037, F-032

## Purpose
The warnings that need types, without waiting for a language server and in the same list as
SPEC-0125, with safe one-keystroke fixes — while staying quiet wherever a type is not known.

## Behaviour
1. **Type model** (per file, on the syntax tree): literals; names declared as parameters (also
   varargs as arrays and catch parameters; multi-catch has no single type), locals (`var` takes
   its initializer's type), enhanced-for and resource variables, fields (also `this.x`); casts,
   `new`, array creation and access; `+` with a String is String, arithmetic by numeric
   promotion, comparisons are boolean; calls to the file's own methods (one return type for the
   name) and to well-known methods of `String`, `StringBuilder`, `BigDecimal`, `BigInteger` and
   the boxes. Types compare without package and generics. Anything else is unknown. [STD]
2. **Rules** (all warnings), reported only when the types involved are known: [GEN, STD]
   - `java.string-equality`: `==`/`!=` between two Strings, or with a string literal on one side
     and not `null` on the other (replaces the SPEC-0098 rule of the same id). An identity
     shortcut — an `if` whose body is only a `return` — is left alone unless a literal is
     compared.
   - `java.boxed-equality`: `==`/`!=` between two values of the same box type.
   - `java.ignored-result`: a statement that calls a value-returning method of `String`,
     `BigDecimal` or `BigInteger` (`trim`, `replace`, `add`, `setScale`, …) and drops the result.
   - `java.equals-incompatible`: `a.equals(b)` between two different types of String,
     StringBuilder, BigDecimal, BigInteger and the boxes (primitives as their boxes).
   - `java.array-to-string`: `arr.toString()`, an array in string concatenation, and
     `String.valueOf(arr)` / `System.out.println(arr)` (also `print`, `System.err`) except `char[]`.
   - `java.array-equals`: `arr.equals(other)`.
   - `java.integer-division`: `/` of two integral values whose result is a `double`/`float` local,
     assignment target or method return value.
3. **Fixes:** `Objects.equals(a, b)` (or `"lit".equals(x)` with a literal, negated for `!=`);
   "Assign the result to 'x'" when the receiver is a plain variable; `Arrays.equals(a, b)`,
   `Arrays.toString(a)` (`deepToString` for nested arrays); `(double) a / b`. `Objects` and
   `Arrays` are written qualified unless `java.util` imports them. Applied like SPEC-0125
   (⌥↩, MCP `fix_inspection`).

## Acceptance criteria
- **AK-1** The type model types 22 expression forms and leaves unknown ones unknown (unit test
  `java_types::tests`); every rule reports its case, stays quiet on the near miss (other types,
  `null`, `char[]`, unknown types, identity shortcut) and every fix gives the expected text (unit
  test `java_inspections::tests::type_aware_rules`).
- **AK-2** No finding of these rules in 10 500 files of real projects (commons-lang, the 5 000-file
  Java and Maven reference projects); checked once with a scan over `~/ReferenceEnv/projects`.
- **AK-3** MCP: an integer division with its fix, applied (`crates/mcp/tests/it/protocol.rs`:
  `inspect_code_tool`).
- **AK-4** Window self-test (`java_inspection_steps`): the ignored `trim()` and the String
  comparison appear as "Ferris IDEA: …" problems; ⌥↩ offers "Assign the result to 'a'" first and
  Enter applies it.
- **AK-5** Hand test (round 4): a real Java project next to the reference.

## Out of scope
Nullability and data flow, generics (e.g. `List<String>.contains(Integer)`), types from other
files and libraries, overload resolution, Kotlin type-aware rules, inspection profiles and
suppression.

## Known deviations from the reference
The reference resolves all types and finds more; Ferris IDEA reports only where the file shows the
types, so it misses cases but does not guess. The identity shortcut `if (a == b) return …;` is not
reported (not compared with the reference).
