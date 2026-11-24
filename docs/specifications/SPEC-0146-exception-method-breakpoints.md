# SPEC-0146: Exception and method breakpoints

- **Version:** 7 · **Date:** 2026-11-15 (v7: a method breakpoint is drawn as a diamond, OBS-0056 P1; v6: ⌘F8 on a Java method header sets a method breakpoint, method entry and exit, method breakpoints switched off, exception breakpoints per language; OBS-0054 B1, B2, B5; v5: Go panics with Delve; v4: one filter for uncaught exceptions, so one exception stops once; agents get the exception's name in the stop reason and can set these breakpoints when attaching; v2: Java exception types and method names through
  java-debug's own ways; v3: field watchpoints and class filters of exception breakpoints,
  OBS-0025 L14)
- **Sources:** GEN (IDE debuggers stop on exceptions, caught or uncaught, optionally only for
  given exception types, and on entry of a method; both are listed in the Breakpoints dialog;
  F-071), STD (Debug Adapter Protocol: `exceptionBreakpointFilters` in the capabilities,
  `setExceptionBreakpoints` with `filters` or `filterOptions` with `condition`
  (`supportsExceptionFilterOptions`), `setFunctionBreakpoints` (`supportsFunctionBreakpoints`),
  `exceptionInfo` (`supportsExceptionInfoRequest`); the filter ids of java-debug
  `caught`/`uncaught`, debugpy `raised`/`uncaught`/`userUnhandled`, js-debug `all`/`uncaught`,
  lldb-dap `cpp_throw`/`objc_throw`/`swift_throw`), SPEC-0039, SPEC-0091, SPEC-0127, IMP-0022,
  ADR-0008, STD v3 (`dataBreakpointInfo`, `setDataBreakpoints`, `supportsDataBreakpoints`), OSS v3
  (java-debug's field ids `pkg.Class#field`, its exception settings `allowClasses`/`skipClasses`
  through jdtls), OBS v3 (OBS-0025 item 14), OBS v6 (OBS-0054: ⌘F8 on a method header line, the small editing popup, exception breakpoints grouped per language)
- **Feature matrix rows:** F-071

## Purpose
Stop where an exception is thrown or a method is entered, without hunting for the line first.

## Behaviour
1. **Breakpoints dialog** (⇧⌘F8): below the line breakpoints, the row "Exceptions: off /
   caught / uncaught / caught and uncaught" and one row per method breakpoint. On the
   exception row "Caught exceptions" (⌥C) and "Uncaught exceptions" (⌥U) switch, the field
   "Exception types" limits them (empty: any). "+ Method Breakpoint" (⌘N) adds a row with the
   fields "Method" and "Method condition"; a method breakpoint without a name is dropped.
   Remove clears the exception row or removes the method breakpoint. Changes are saved at
   once in `.ferris-idea/special-breakpoints.json`. [GEN]
2. **Mapping:** "caught" means every thrown exception and uses the adapter's filter for that
   (`caught`, `raised`, `all`, `cpp_throw`, `objc_throw`, `swift_throw`); "uncaught" uses
   `uncaught`, `userUnhandled` or `unhandled`. If a "throw" filter is on, uncaught ones stop
   too. **(v4)** For "uncaught" exactly one filter is switched on: `uncaught` where the adapter
   has it, else `unhandled`, else `userUnhandled`. debugpy offers `uncaught` and
   `userUnhandled`; with both on, one exception nobody catches stopped the program twice
   (tried 2026-11-13 with debugpy 1.8.21). With exception types the filters are sent as `filterOptions` with the types as
   condition, only where the adapter supports that. java-debug takes the types (separated by
   commas or spaces) from its debug settings: they are set with jdtls's
   `vscode.java.updateDebugSettings` (`exceptionFilters.exceptionTypes`) before the filters are
   sent, and cleared when the field is emptied (v2, hand test 72, D30: refused before). [STD, OSS]
3. **Method breakpoints** are function breakpoints in the adapter's syntax (`add`,
   `Shapes::area`, `com.acme.Shop.total`), optionally with a condition. For java-debug a written
   `com.acme.Shop.total` or `Shop::total` is sent as `com.acme.Shop#total`, the form it reads (v2,
   D30: the method breakpoint never stopped in Java). [STD, OSS]
4. **Applying:** sent before the program runs and again whenever the dialog changes them while
   a session runs. What the adapter cannot do (no such filter, no function breakpoints, no
   type limit) is said in the debug output as "[Ferris IDEA] the debug adapter …"; the session
   and its line breakpoints go on (IMP-0022: never silently ignored). [STD]
5. **Stop on an exception:** the reason reads "exception: <type or description>" from
   `exceptionInfo` where the adapter answers it. [STD] **(v4)** Also in the state the MCP tools
   return (`debug_start`, `debug_attach`, `debug_resume`, `debug_pause`, `debug_run_to_line`);
   before, agents got the bare word `exception`.
6. **MCP:** `saved_breakpoints` also returns `special`; `set_special_breakpoints` changes them
   (dry run by default); `debug_start` takes `exception_breakpoints`, `exception_types` and
   `function_breakpoints`. [ADR-0008] **(v4)** `debug_attach` takes the same three; `debug_start`
   with `saved_breakpoints: true` starts from the saved ones (SPEC-0127 v6 behaviour 16), and
   `exception_breakpoints: "none"` switches the exception breakpoints off for that start.

7. **Field watchpoints (v3):** "+ Field Watchpoint" (⇧⌘N) in the dialog adds a row "Field" (`pkg.Class.field`
   or java-debug's `pkg.Class$Inner#field`; a written `pkg.Class.field` is sent as
   `pkg.Class#field`), "Stop on" (write, read, read and write) and "Field condition". The
   variables menu's "Add Field Watchpoint" on a field asks the adapter for the field's id
   (`dataBreakpointInfo`) and adds that row. They are DAP data breakpoints, sent with the other
   special breakpoints; a stop reads "data breakpoint". Adapters without data breakpoints (debugpy,
   js-debug) refuse with the console line of behaviour 4; a field without a name is dropped. Saved
   in `special-breakpoints.json` as `fields`. [STD, OSS, OBS]
8. **Class filters of exception breakpoints (v3):** on the exception row "Exception classes" limits
   the stops to exceptions thrown in classes matching its patterns (`dbg.*`; `-dbg.Test`
   excludes); java-debug takes them from its debug settings (`allowClasses`, `skipClasses`) with
   the types. Other adapters refuse with a console line. Saved as `exception_classes`. [OSS, OBS]
9. **java-debug's settings (v3):** every update of java-debug's debug settings carries a log
   level; java-debug 0.53 refused updates without one (the types of v2 too). [OSS]
10. **MCP (v3):** `set_special_breakpoints` takes `exception_classes` and `fields` (field, access,
   condition); `debug_field_watch` watches a field of a stopped session (SPEC-0091 v12). [ADR-0008]
11. **Go (v5):** Delve offers two filters, a panic that nothing recovers (`unrecovered-panic`) and
    a fatal error of the runtime (`runtime-fatal-throw`); both stand for "uncaught" and both are switched on, other than the
    one filter of behaviour 2: one event is never both, so nothing stops twice. The stop's
    reason carries the panic's message from `exceptionInfo`. Delve has no stop on every panic, so
    "caught" is refused with the console line of behaviour 4, and exception types and classes
    likewise. Method breakpoints are Delve's function breakpoints (`calc.Add`,
    `main.(*Shop).Total`), with a condition as a Go expression. While no exception or method
    breakpoint is set, Delve's own default applies, which stops on both. [OSS: Delve's public
    documentation; tried with Delve 1.27.2]

12. **⌘F8 on a method header (v6, OBS-0054 B1):** Toggle Line Breakpoint (⌘F8, Ctrl+F8) on the
    line that holds the name of a Java method declaration (constructors not) toggles a method
    breakpoint instead of a line breakpoint: `pkg.Type.method`, with the file's package and the
    simple name of the type that declares the method. It remembers its file and the 0-based line
    (`file`, `line` in `special-breakpoints.json`) so the gutter shows it and a second ⌘F8 on that
    line removes it. The gutter draws it as a red diamond (v7, OBS-0056 P1; until v6 a square), a switched-off one as a red outline diamond. Other lines and other languages keep
    line breakpoints. The dialog lists it with the other method breakpoints. [OBS, GEN]
13. **Method entry and exit (v6, OBS-0054 B2):** a method breakpoint watches the entry (on by
    default), the exit or both; at least one stays on. Entry is the adapter's function
    breakpoint. Exit is Ferris IDEA's own, since the protocol only stops on entry: at the entry hit
    of a breakpoint that watches only the exit, Ferris IDEA steps out of the method and reports the
    stop after it returned with the reason `method exit`; with both, the program stops at entry,
    and the next Resume of that thread first steps out so it stops again after the return
    (`method exit`), then Resume goes on as usual. Saved as `entry` (only when off) and `exit`.
    [STD, GEN]
14. **Switched off (v6, OBS-0054 B3):** a method breakpoint can be switched off without removing it
    (`disabled`); a switched-off one is not sent to the adapter. [GEN]
15. **Exception breakpoints per language (v6, OBS-0054 B5):** the dialog groups the exception
    breakpoints by the languages Ferris IDEA debugs — Java (also Kotlin), Python, JavaScript (also
    TypeScript), Native (C, C++, Rust, Swift) and Go — each group a heading with one row "Any
    exception" carrying caught, uncaught, exception types and exception classes of that language.
    A session uses the row of its adapter's language (java-debug and kotlin-debug-adapter: Java;
    debugpy: Python; js-debug: JavaScript; lldb-dap: Native; Delve: Go). Settings saved before v6,
    one row for all, hold for every language that has no row of its own; saved as `languages`
    (language → caught, uncaught, exception_condition, exception_classes). [OBS, GEN]
16. **MCP (v6):** `set_special_breakpoints` takes `language` (`java`, `python`, `javascript`,
    `native`, `go`) for the exception settings of that language only (without it, the shared
    setting as before), and per function breakpoint `entry`, `exit`, `enabled`, `file` and `line`;
    `saved_breakpoints` returns them in `special`. [ADR-0008]

## Acceptance criteria
- **AK-1** Filter mapping for java-debug, debugpy, js-debug and lldb-dap ids, refusals
  (`session::tests::exception_filters_by_adapter`); saving and loading
  (`saved::tests::special_breakpoints_round_trip`).
- **AK-2** With `lldb-dap`: a function breakpoint on `add` stops on entry; a C++ `throw` with
  caught exceptions on stops with reason `exception` and exception info
  (`crates/debugger/tests/it/lldb.rs`: `function_and_exception_breakpoints_with_lldb_dap`).
- **AK-3** MCP: `saved_breakpoints` `special`, `set_special_breakpoints` dry run and saving,
  `debug_start` dry run with exception and function breakpoints (`saved_breakpoint_tools`).
- **AK-4** Window self-test: in the Breakpoints dialog ⌥U on the exception row, ⌘N, a method
  name and Enter save uncaught exceptions and the method breakpoint.
- **AK-5** Hand test (round 4, no. 72): Java (NullPointerException, a method breakpoint) and
  Python (uncaught exception) next to the reference.
- **AK-8 (v4)** One filter for uncaught (`session::tests::exception_filters_by_adapter`); with real
  debugpy an exception nobody catches stops once, the reason starting with `exception: ` and
  naming `ValueError`, then the program ends
  (`crates/mcp/tests/it/protocol.rs`: `python_breakpoints_change_while_the_program_runs`, skipped
  without debugpy); the saved exception breakpoints in a start and `none`
  (`debug_start_takes_the_saved_breakpoints`).
- **AK-6 (v3)** With real jdtls and java-debug (`crates/workbench/tests/it/java_debug_more.rs`): the
  variables' field `c.n` gives the id `Hot$Counter#n`, and the watchpoint stops on line 16 where
  the loop writes it; the settings carry the log level (`java_debug::tests`). MCP
  (`saved_breakpoint_tools`): fields and exception classes saved, a field without name dropped.
- **AK-7 (v3)** Window self-test `java_debug_more_steps`: Add Field Watchpoint saves the field and
  the program stops there (written, not yet run in the window).
- **AK-9 (v5)** With `dlv` and `go` (`crates/debugger/tests/it/delve.rs`:
  `go_panic_function_breakpoint_and_build_error`): a function breakpoint with a condition stops on
  entry, a write to a nil map stops with reason `exception` and its message, and "caught" is
  refused in the output while the program runs to its end. Unit test:
  `session::tests::exception_filters_by_adapter` with Delve's filter ids.

- **AK-v6-1** The method on a header line, qualified with the package, none on a body line or a
  constructor (`java_structure::tests::method_named_on_line`); saving and reading `file`, `line`,
  `entry`, `exit`, `disabled` and `languages` (`saved::tests::special_breakpoints_round_trip`); the
  exceptions a session of each adapter gets (`session::tests::exceptions_per_language`).
- **AK-v6-2** Against a fake adapter (`crates/debugger/tests/it/method_exit.rs`): exit only — the
  entry hit steps out and the wait ends with `method exit` in the caller; entry and exit — the
  stop at entry, then Resume steps out and stops with `method exit`, the next Resume runs on; a
  switched-off method breakpoint is not sent.
- **AK-v6-3** In a window of the test harness with real keys (`crates/workbench/tests/it/breakpoint_popup_keys.rs`:
  `cmd_f8_on_a_method_header_sets_a_method_breakpoint`): ⌘F8 on the header of `Cart.total` saves
  `shop.Cart.total` with its file and line and the gutter shows the method look; ⌘F8 on a body
  line sets a line breakpoint; ⌘F8 on the header again removes the method breakpoint. In the
  dialog (`exception_breakpoints_per_language_and_row_checks`): the headings of the five
  languages, ⌥U on the Python row sets uncaught for Python only, saved under `languages`.
- **AK-v7-1** The diamond's corners lie on the middle of the mark's edges, filled for a method
  breakpoint that is on and outlined for one that is off (`editor_view::tests::method_breakpoint_diamond`).
- **AK-v6-4** MCP (`crates/mcp/tests/it/protocol.rs`: `special_breakpoints_per_language_and_method_watch`).

## Out of scope
Caller filters of the reference's Java exception breakpoints, data breakpoints on memory,
disabling single field watchpoints, gutter marks for field watchpoints and for method breakpoints
added in the dialog by name (v6: ⌘F8 on a header gives one), method breakpoints from ⌘F8 in
Kotlin and other languages, instance filters of field watchpoints.

## Known deviations from the reference
Method breakpoints use the adapter's function breakpoints; the reference's Java method
breakpoints can also stop on exit. The exception types condition is adapter syntax. **(v5)** Go
has no stop on a recovered panic, because Delve offers none.
