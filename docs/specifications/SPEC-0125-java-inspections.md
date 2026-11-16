# SPEC-0125: Java inspections with quick fixes

- **Version:** 3 · **Date:** 2026-11-06 (v2: four more rules, suppression, the options of a
  finding in ⌥↩ with "fix all in file", previews of own fixes — OBS-0019 items 1, 4; v3: run the
  inspection on a scope, disable highlighting but keep the fix, suppress all for the class — defect
  F032-2)
- **Sources:** GEN (IDEs mark common Java mistakes and leftovers the compiler accepts — unused
  imports, locals and private members, comparisons with boolean literals, `size() == 0`,
  self-assignment, empty bodies, constant and repeated conditions, `printStackTrace()`, deprecated
  boxing constructors, `equals` without `hashCode` — and fix them from ⌥↩ / Alt+Enter; F-032),
  STD (Java Language Specification; `Integer.valueOf` and friends, boxing constructors deprecated
  since Java 9; `String.isEmpty()` since Java 6, `CharSequence.isEmpty()` only since Java 15),
  SPEC-0098 (own inspections), SPEC-0076 (context actions), ADR-0008; v2: OBS-0019 items 1, 4, OSS
  (`JetBrains/intellij-community`: the short names of the matching inspections, used as suppression
  ids, and the `//noinspection` comment form), STD (`@SuppressWarnings`, JLS 9.6.4.5); v3: OBS-0019
  item 4 (the submenu's further entries), OSS (`intellij-community`: the profile level
  `INFORMATION` of "no highlighting, only fix", the suppression name `ALL`), SPEC-0098 v4, SPEC-0315 v2
- **Feature matrix rows:** F-032

## Purpose
Java code gets the everyday warnings people expect from the reference, and the safe ones can be
fixed with one keystroke — in the editor and by agents.

## Behaviour
1. **Rules** (on the syntax tree, as in SPEC-0098; level weak for leftovers, warning for likely
   bugs): [GEN, STD]
   - `java.unused-import` (weak): a single-type or static import whose simple name is not used in
     the file and not mentioned in a comment (Javadoc `{@link}`); wildcard imports are left alone.
   - `java.unused-local` (weak): a local variable that is never read later in its block.
   - `java.unused-private` (weak): a private field or method never used in the file; members with an
     annotation and the serialization methods (`readObject`, `writeObject`, `readResolve`,
     `writeReplace`, `readObjectNoData`) count as used.
   - `java.boolean-literal-comparison` (weak): `x == true`, `false != x` and the like.
   - `java.size-zero` (weak): `size() == 0`, `length() > 0` and the like, only when the receiver is
     declared in the file as `String` (for `length()`) or as a type named `…List`, `…Set`, `…Map`,
     `…Collection`, `…Queue` or `…Deque` (for `size()`), so `CharSequence` and unknown types stay
     quiet.
   - `java.self-assignment`: `x = x`, `this.x = this.x`.
   - `java.empty-if`: `if (…);` — the `;` ends the statement.
   - `java.constant-condition`: `if (true)`, `if (false)`, `while (false)`; `while (true)` is fine.
   - `java.duplicate-condition`: a condition repeated along one `if` / `else if` chain (compared
     without whitespace).
   - `java.print-stack-trace` (weak): `e.printStackTrace()` without arguments.
   - `java.boxing-constructor` (weak): `new Integer(…)`, `new Long(…)`, … (all eight boxes).
   - `java.empty-block`: an empty `finally` or `synchronized` block (no comment inside).
   - `java.equals-hash-code`: a class that declares `equals(Object)` but not `hashCode()`, or the
     other way round.
2. **Quick fixes** where the change is safe without type information: [GEN]
   - boolean literal → "Simplify to 'x'" / "Simplify to '!x'" (parentheses where needed);
   - size compared with zero → "Replace with 'x.isEmpty()'" / "'!x.isEmpty()'";
   - boxing constructor → "Replace with 'Integer.valueOf(…)'";
   - self-assignment → delete the statement; unused import → "Remove unused import" (whole line);
   - unused local → "Remove variable 'x'" only when it is the declaration's single variable and its
     initializer is a literal or a plain name, so no call with side effects disappears.
3. **⌥↩ / Alt+Enter** (SPEC-0076) lists IDECraft's fixes for findings at the caret first, before the
   language server's actions (same titles only once); with own fixes at hand the list waits at most
   3 s for the language server and also shows without it. Enter applies the fix as one undo step; if
   the text changed after the list opened, nothing is applied and the status bar says so. The list
   also shows when the caret line is scrolled out of view (next to the nearest visible line), and a
   late answer to an earlier ⌥↩ never replaces a newer list. [GEN]
4. **Inspect Code** and the editor show the new rules like those of SPEC-0098.
6. **More rules (v2)** (on the syntax tree, within the file): [OBS-0019, GEN]
   - `java.redundant-cast` (warning): a cast `(T) x` where `x` is a plain name whose declaration in
     the file (local variable, parameter or field, the nearest in scope) has exactly the type `T`
     (compared without white space); fix "Remove redundant cast" replaces the cast by `x`.
   - `java.field-may-be-final` (warning): a `private` field without `final`, with an initializer,
     that is never assigned in the file (no `=`, compound assignment, `++` or `--` on its name or on
     `this.` plus its name) and not `volatile`; fix "Add 'final' modifier".
   - `java.unused-parameter` (weak, grey as unused code in SPEC-0070 v5): a parameter of a `private`
     method or constructor that its body never mentions; no fix (callers would change).
   - `java.null-dereference` (warning): a local variable initialised with `null` or with a conditional
     expression one of whose branches is `null`, whose next mention in the same block is the
     receiver of a method call or field access (`x.m()`, `x.f`) — reported on the member name; a
     comparison with `null`, an assignment or any other mention first keeps it quiet; no fix.
   - `java.unused-local`, `java.unused-import`, `java.unused-private` and `java.unused-parameter` are
     marked as unused code (SPEC-0070 v5 behaviour 6).
7. **Suppression (v2):** a finding is not reported inside code that suppresses it: [OSS, STD]
   - a line comment `//noinspection Name[,Name…]` directly above a statement, local declaration,
     field, method or class suppresses those inspections in that element;
   - `@SuppressWarnings("Name")` or `@SuppressWarnings({"A", "B"})` on a class, method, constructor,
     field or local declaration suppresses them inside it.
   A name matches an inspection by its IDECraft id (`java.string-equality`), by its short name below,
   `unused` matches the four unused rules and `ALL` every rule. Short names: `java.string-equality`
   StringEquality, `java.empty-catch` CatchMayIgnoreException, `java.unused-import` UNUSED_IMPORT,
   `java.unused-local`/`-private`/`-parameter` unused, `java.redundant-cast` RedundantCast,
   `java.field-may-be-final` FieldMayBeFinal, `java.null-dereference` DataFlowIssue,
   `java.boolean-literal-comparison` PointlessBooleanExpression, `java.size-zero`
   SizeReplaceableByIsEmpty, `java.self-assignment` SillyAssignment, `java.empty-if`
   EmptyStatementBody, `java.constant-condition` ConstantValue, `java.duplicate-condition`
   DuplicateCondition, `java.print-stack-trace` ThrowablePrintStackTrace, `java.boxing-constructor`
   CachedNumberConstructorCall, `java.empty-block` EmptyFinallyBlock or EmptySynchronizedStatement
   (by the block), `java.equals-hash-code` EqualsAndHashcode (registrations in
   `java/java-backend/resources/META-INF/InspectionGadgets.xml` and `Inspections.xml`).
8. **Options of a finding in ⌥↩ (v2):** every own fix row in the ⌥↩ list (SPEC-0076) has a submenu,
   opened with → or a click on its ›, closed with ← or Escape; a finding without a fix gets its own
   row named after the inspection, with the same submenu (Enter opens it). The submenu offers: [OBS-0019]
   - "Fix all '<inspection name>' problems in file" (only with a fix): applies the fix of every finding
     of that inspection in the file as one undo step; overlapping edits are skipped;
   - "Suppress for statement" (Java): inserts `//noinspection <short name>` on its own line above the
     statement or member, indented like it;
   - "Suppress for method" and "Suppress for class" (Java): adds the short name to an existing
     `@SuppressWarnings` of the enclosing method (or constructor) or class, or inserts
     `@SuppressWarnings("<short name>")` on its own line above it;
   - "Disable inspection": turns the inspection off in the project's profile (SPEC-0315);
   - "Edit inspection settings": opens Settings › Editor › Inspections with the inspection selected.
   Each is one undo step and the problems refresh at once.
   **v3** adds three entries; the order of the submenu becomes: fix all, **"Run inspection on…"**,
   suppress for statement, method, class, **"Suppress all inspections for class"** (Java),
   **"Disable highlighting, keep fix"** (only with a fix, and not when the inspection is already at
   that level), disable inspection, edit inspection settings: [OBS-0019, OSS]
   - "Run inspection on…" opens Inspect Code's scope dialog (SPEC-0098 v3 behaviour 7) titled with
     the inspection's name; Analyze runs only that inspection over the chosen scope (SPEC-0098 v4
     behaviour 10);
   - "Suppress all inspections for class" adds `ALL` to the enclosing class's `@SuppressWarnings` or
     inserts `@SuppressWarnings("ALL")` above it, so no inspection reports inside the class; not
     offered when the class already suppresses `ALL`;
   - "Disable highlighting, keep fix" sets the inspection to the level "No highlighting, only fix"
     in the project's profile (SPEC-0315 v2): its findings leave the editor's marks, counts and the
     Problems tool window, but ⌥↩ at their place still lists their fix with this submenu.
9. **Look of the ⌥↩ list (v2):** a thin separator divides the quick fixes (own fixes and the server's
   quick fixes) from the other actions. The preview (F1, SPEC-0076) of an own fix shows the lines it
   changes, before and after, like the preview of the server's actions (IMP-0307). [OBS-0019]
10. **MCP:** `inspect_code` reports the Java rules with `fix` (the fix title, if any);
   `fix_inspection` (path, 0-based line and column, id; `dry_run` default true) returns the fixed
   text and writes it with `dry_run=false`, refusing a file that changed meanwhile. v2: with
   `all=true` it fixes every finding of that inspection in the file; `suppress_inspection` (path,
   line, column, id, `scope` statement, method or class; `dry_run` default true) returns or writes
   the text with the suppression of behaviour 8. v3: `suppress_inspection` takes `scope=all` for
   "Suppress all inspections for class"; `inspection_profile` takes the severity `info` for
   "Disable highlighting, keep fix" (SPEC-0315 v2); `inspect_code` takes `id` to run one inspection
   (SPEC-0098 v4). [ADR-0008]

## Acceptance criteria
- **AK-1** Every rule reports its case and stays quiet on the near miss; every fix produces the
  expected text (unit tests `java_inspections::tests`).
- **AK-2** MCP: Java findings with fix titles, a `fix_inspection` preview, applying it, an unknown
  finding refused (`crates/mcp/tests/it/protocol.rs`: `inspect_code_tool`).
- **AK-3** Window self-test (`java_inspection_steps`): an unused import, an unused local and
  `ready == true` appear as "IDECraft: …" problems; ⌥↩ offers "Simplify to 'ready'" first and Enter
  applies it.
- **AK-4** Hand test (round 4): a real Java project next to the reference.
- **AK-5 (v2)** Every new rule reports its case and stays quiet on the near miss; suppression by
  comment and annotation, by id, short name, `unused` and `ALL`; the suppression edits for statement,
  method (with and without an existing annotation) and class; "fix all" over two findings (unit tests
  `java_inspections::tests`).
- **AK-6 (v2)** MCP: `fix_inspection` with `all`, `suppress_inspection` as a preview and written
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-7 (v2)** Window self-test (`java_inspection_steps`): → on an own fix opens its submenu, "Suppress
  for statement" inserts the comment and the finding disappears, ⌘Z restores it; side by side in
  OBS-0019 (re-check).

- **AK-8 (v3)** The submenu lists the three new entries in the order of behaviour 8 (unit test
  `intentions_model::tests::finding_options_and_own_fix_preview`); "suppress all" inserts and extends
  `@SuppressWarnings` with `ALL` and the class reports nothing after it (unit test
  `java_inspections::tests::suppress_all_for_class`); a finding at "no highlighting, only fix" is not
  shown in the editor but its fix is still offered (unit test `inspections::tests::fix_only_keeps_the_fix`);
  MCP `suppress_inspection` with `scope=all` (`crates/mcp/tests/it/protocol.rs`:
  `suppress_inspection_tool`); window self-test (`java_inspection_steps`, written): "Disable
  highlighting, keep fix" on `==` removes its mark, ⌥↩ there still offers the fix; "Run inspection
  on…" opens the scope dialog and Analyze lists only that inspection.

## Out of scope
Rules that need types across files (nullability of other methods' results, resource leaks,
unchecked casts), Kotlin rules beyond SPEC-0098.

## Known deviations from the reference
The reference resolves types; IDECraft decides on the syntax tree and therefore reports
`size() == 0` only for receivers whose declared type is known from the file, and offers removal of
an unused local only when that cannot drop a side effect. v2: the null-dereference rule only follows
a local variable within its block; the reference also follows method results and fields. Undoing
"fix all" is one undo step without a confirmation (IMP-0306). Improvement: the fixes are also available
to agents, headless and as a preview first (IMP-0046). v3: "Disable highlighting, keep fix" is
offered only for findings with a fix, because without one it would only hide the finding (turning
the inspection off says that more plainly).
