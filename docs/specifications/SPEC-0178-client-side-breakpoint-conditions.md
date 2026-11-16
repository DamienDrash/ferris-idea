# SPEC-0178: Breakpoint conditions evaluated by IDECraft

- **Version:** 3 · **Date:** 2026-11-05 (v2: own evaluation of simple expressions, behaviour 4a; v3:
  log messages in order with the program's output)
- **Sources:** GEN (a debugger offers conditions, pass counts and logging breakpoints for every
  language it debugs; F-037), STD (DAP: the capabilities `supportsConditionalBreakpoints`,
  `supportsHitConditionalBreakpoints`, `supportsLogPoints`; the `stopped` event, `stackTrace`,
  `evaluate`, `continue`; hit conditions as `N`, `>N`, `>=N`, `<N`, `<=N`, `==N`, `%N`), OSS
  (`fwcd/kotlin-debug-adapter` 0.4.4 announces none of the three capabilities and ignores
  conditions; upstream issue fwcd/kotlin-debug-adapter#97, open; its `evaluate` resolves only a
  variable name and answers `unknown` for any other expression, tried 2026-11-03), SPEC-0039 v2, SPEC-0127,
  SPEC-0132, ADR-0008
- **Feature matrix rows:** F-037 (and every other language whose adapter lacks the capabilities)

## Purpose
Conditions, pass counts and logging breakpoints work in every debug session, also with adapters that
cannot evaluate them themselves (kotlin-debug-adapter). The user sees when IDECraft does the work and
that it is slower.

## Behaviour
1. **Who evaluates:** for each of the three features separately, an adapter that announces the
   capability gets the feature in `setBreakpoints` as before and IDECraft does nothing else. When
   the adapter does not announce it, IDECraft sends the breakpoint without that part and keeps it
   to evaluate itself. A breakpoint reports which of the two applies (`evaluated_by`: `adapter` or
   `idecraft`). [STD]
2. **Recognising the hit:** a `stopped` event with reason `breakpoint` (or with
   `hitBreakpointIds`) whose thread's top frame is in the same file (compared after resolving
   symbolic links) and on the line of a breakpoint kept by IDECraft is that breakpoint's hit. Stops
   for other reasons (step, pause, exception, entry) are never evaluated or skipped. [STD]
3. **Order per hit:** first the condition; when it is false the program continues and the hit does
   not count. Then the hit counter of the breakpoint goes up by one and the hit condition is
   checked (`N` and `==N`: only the N-th hit; `>N`, `>=N`, `<N`, `<=N`: compared; `%N`: every N-th);
   not met → continue. Then a log message, if any: each `{expression}` part is evaluated in the
   top frame, the line goes to the debug console and the program continues. Otherwise the program
   stops as at a plain breakpoint. [STD, GEN]
4. **Values:** the condition is evaluated in the top frame of the stopped thread (`evaluate`,
   context `watch`). `true`/`True` and numbers other than zero are true; `false`/`False`, `0`,
   `null`, `nil`, `None`, `undefined` are false. [GEN]
4a. **Own evaluation:** when the adapter's answer is an error or not a truth value, IDECraft
   evaluates the expression itself if it is simple: literals (whole and decimal numbers, strings
   in `"…"`, characters in `'…'`, `true`, `false`, `null`), names and field paths (`order.total`),
   parentheses, `!` and unary `-`, `* / %`, `+ -` (`+` joins text), the comparisons
   `== != === !== < <= > >=` and `&& ||` with the usual precedence and short-circuit. Each name or
   path is asked from the adapter (`evaluate`); when the adapter cannot answer it, it is looked up
   in the top frame's local variables and their children. Whole numbers divide as whole numbers;
   text compares by content. Anything else (calls, indexing, operators not listed) fails as in
   behaviour 5, naming both reasons. [STD, GEN]
5. **Failures stop the program:** when the condition cannot be evaluated (the adapter answers with
   an error) or gives something that is not a truth value, the program stops at that hit and the
   debug console says which condition at which file and line failed and why; an unreadable
   `{expression}` in a log message is written as `<error: …>` and the program continues. An
   unknown hit condition syntax is refused when the breakpoint is set. [GEN]
6. **Visible:** when a session keeps at least one breakpoint for IDECraft to evaluate, the debug
   console says once, at the start or when such a breakpoint is first set, that the adapter cannot
   evaluate them, that IDECraft evaluates them at each hit, and that this is slower because the
   program stops briefly every time the line is reached. The Breakpoints dialog says the same under
   the condition field for Kotlin files. Log messages and these notes stand in the console in the
   order they happen, between the program's output from before and after them (v3, hand test 54,
   D17: they came after the program's later output). [GEN]
7. **Time:** skipped hits are handled inside the wait for the next stop; a wait that ends while
   IDECraft is skipping hits reports the program as running. Hit counters belong to the session;
   they start again when the breakpoint's condition or hit condition changes. [GEN]
8. **Function breakpoints** keep their behaviour (SPEC-0146): a condition on a function breakpoint
   for an adapter without the capability is still refused with the reason. [STD]
9. **MCP:** `debug_start` no longer refuses conditions, hit counts and logpoints for such adapters;
   its `breakpoints` show `evaluated_by` per breakpoint and `console` carries the note of
   behaviour 6 and the log lines. [ADR-0008]

## Acceptance criteria
- **AK-1** With a fake adapter without the capabilities: a condition stops only on the matching
  iteration and the adapter never received the condition; a hit condition `3` and `%2` stop on the
  right hits; a logpoint writes each line to the console and never stops; a condition that fails to
  evaluate stops with the reason in the console; a stop by stepping onto the line is not evaluated;
  with an adapter that resolves only names (like kotlin-debug-adapter), `i % 2 == 0 && i > 2`
  is evaluated by IDECraft (`crates/debugger/tests/it/client_conditions.rs`); the expression
  evaluator on its own (`expr::tests`).
- **AK-2** With a fake adapter that announces the capabilities: the condition goes to the adapter
  and IDECraft sends no `evaluate` (same file); unit tests for truth values and hit conditions
  (`session::tests`).
- **AK-3** With the real kotlin-debug-adapter and a Gradle project built offline: the condition
  `x == 2` stops once with `x = 2`, then the program ends with `total 12`; skipped cleanly without
  the adapter (`crates/debugger/tests/it/kotlin.rs`).
- **AK-4** MCP: `debug_start` reports `evaluated_by` (`crates/mcp/tests/it/protocol.rs` or a session
  unit test of the JSON shape).
- **AK-5** Hand test (round 4): a conditional breakpoint and a logpoint in a Kotlin program in the
  window, the console note visible.

## Out of scope
Conditions on function and exception breakpoints evaluated by IDECraft, making skipped hits faster
(each one is a stop and a resume over the protocol), counting hits of a breakpoint whose line the
adapter moved (matched by the line the adapter reports, else the line asked for).

## Known deviations from the reference
The reference evaluates conditions inside its own JVM debugger without a round trip per hit;
IDECraft is slower with kotlin-debug-adapter and says so. Not compared side by side.
