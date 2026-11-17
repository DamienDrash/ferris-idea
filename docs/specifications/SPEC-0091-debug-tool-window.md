# SPEC-0091: Debugging in the window

- **Version:** 22 · **Date:** 2026-11-16 (v22 (v20 and v21 are taken by #891 and #902): a value with line breaks after a line of code is put on one line too, behaviour 13, AK-27; hand test 20 of the beta of workflow (a); v20: Jump to Type Source from the Variables list, behaviour 45, AK-28 (AK-25 is taken by v13, AK-26 by #902, AK-27 by v22); eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v19: Quick Evaluate Expression, behaviour 44, AK-24; eighth package of docs/research/KEYMAP_COVERAGE.md; v18: Go files and Go tests through Delve, SPEC-0039 v7; v17: a breakpoint the session cannot bind is hollow, SPEC-0127 v5, OBS-0035 L14; v16: JVM values in one form with the size of collections, no `this` row in a frame without a receiver, OBS-0035 L11 and L12; v15: values at the end of a line are placed one after the other; v14: a value with line breaks stays on one row; v2: Python scripts through debugpy; v3: evaluate expression,
  breakpoints kept across restarts; v4: Node.js scripts through js-debug; v5: watches, threads, values
  in the editor; v6: the field's hint is a placeholder; v7: debugger items in the editor menu, the
  adapter's echo of the command line, closure scopes; v8: values in the editor also for the fields of the
  frame's object, OBS-0014; v9: the frame's object with its text, the fields the method uses at the
  top of the variables, OBS-0014; v10: threads in a selector, library frames folded, OBS-0014; v11: ⌃D/⌃R on "Current File", object texts
  of Java values, Smart Step Into, Reset Frame, forced steps, the toolbar's ⋮ menu, completion in the
  expression field, the menus of frames and variables, Mark Object, OBS-0025; v12: the rest of
  OBS-0025 L5, L7, L8, L9b, L12, L15, L17 — Step Out of Code Block, thread dump and states, Resume
  and Pause All, Force Return and Throw Exception refused, Inspect, referring objects, views,
  renderers, filters, mark colours, code fragments, hot swap, the console's side bar and input;
  v13: the window check next to the reference, OBS-0025 re-check 2 — sleeping threads, Escape on the
  thread list, views that fit the value, Hex in capitals, values in the editor in their view, the
  mark field keeps the keyboard, hot swap refused while a changed source has errors, the platform's
  console icons)
- **Sources:** GEN (line breakpoints toggled in the gutter, a Debug action for the selected run
  configuration, a Debug tool window with stepping toolbar, call stack, variables and console, the
  execution point highlighted in the editor; F-070), OSS (action ids `ToggleLineBreakpoint`, `Debug`,
  `Resume`, `StepOver`, `StepInto`, `StepOut`, `Stop`, `ActivateDebugToolWindow`, `EvaluateExpression` with
  their shortcuts in
  the ported keymaps; scheme keys `BREAKPOINT_ATTRIBUTES`, `EXECUTIONPOINT_ATTRIBUTES`; platform icons
  `expui/breakpoints/*`, `expui/run/{debug,resume,stepOver,stepInto,stepOut}`, `expui/toolwindows/debug`,
  `expui/debugger/{frame,dbPrimitive,dbObject}`), STD (Debug Adapter Protocol), SPEC-0039 (DAP session),
  SPEC-0054 (debugging through MCP), SPEC-0087 (run configurations), OBS (OBS-0014: Java side by side;
  OBS-0025: Java and Python side by side, stepping, evaluating, variables), OSS v11 (action ids
  `SmartStepInto` ⇧F7, `ForceStepInto` ⌥⇧F7, `ForceStepOver` ⌥⇧F8, `ForceRunToCursor` ⌥⌘F9,
  `ShowExecutionPoint` ⌥F10, `Debugger.PopFrame`, `Debugger.MarkObject` in the ported keymaps and
  the platform's action list), STD v11 (DAP `stepInTargets`, `stepIn` with `targetId`,
  `restartFrame`, `completions` with their capabilities), third-party open source v11 (java-debug's
  lazy values with the object's text), OSS v12 (action ids `StepOutOfBlock`, `DumpThreads`,
  `Debugger.ForceEarlyReturn`, `Debugger.ThrowException`, `Hotswap`, `PreviousOccurence`,
  `NextOccurence` of the platform's action list and keymaps), STD v12 (DAP `evaluate` contexts
  `watch`/`repl`, `evaluateName`, `dataBreakpointInfo`, `setDataBreakpoints`, `continue` of every
  thread), third-party open source v12 (java-debug's `pauseOthers`, `continueAll`, `pauseAll`,
  `redefineClasses` with its `hotcodereplace` events, statements in `evaluate`, debug settings with
  `showStaticVariables` through jdtls; debugpy's `repl` context, which runs statements), OBS v16
  (OBS-0035 item 18: Kotlin debugging side by side), STD v16 (DAP `variables` with
  `indexedVariables` and `namedVariables`), third-party open source v16 (the value texts of
  `fwcd/kotlin-debug-adapter` 0.4.4 and java-debug, tried 2026-11-11: kotlin-debug-adapter sends a
  local `this` with the value `null` and the type "Unknown type" in a frame without a receiver,
  writes objects as `instance of <class>(id=<n>)` and arrays as `instance of <type>[<n>] (id=<n>)`,
  gives no child counts, and its `evaluate` reads `names.size` as a field but answers `unknown`
  for a call; java-debug writes `ArrayList@13 size=1` and gives `indexedVariables`)
- **Feature matrix rows:** F-070

## Purpose
Stop a program at a line, look at its call stack and variables, and step through it without leaving
the editor.

## Behaviour
1. **Breakpoints:** a click on a line number, or `ToggleLineBreakpoint` (⌘F8 on macOS, Ctrl+F8
   elsewhere) on the caret line, sets or removes a line breakpoint. A breakpoint shows as a red dot in
   place of the line number, and its line gets the scheme's `BREAKPOINT_ATTRIBUTES` background.
   Breakpoints move with edits: lines below an inserted or removed line break shift, and a breakpoint
   on a line that disappears moves to the line where the edit happened. While a session runs, every
   change is sent to it. **(v17)** While a session runs, a breakpoint it reports as not bound is
   drawn hollow with the reason in its tooltip, and filled again once bound or when the session
   ends (SPEC-0127 v5 behaviour 14). [GEN, OSS]
2. **Debug:** `Debug` (⌃D on macOS, ⇧F9 elsewhere) starts the selected run configuration (SPEC-0087)
   under lldb-dap, with the breakpoints of all open editors. The program is the configuration's first
   word: relative to its working directory when it contains a `/`, otherwise looked up on the `PATH`.
   The other words are the arguments, and the environment comes from the configuration. Without
   lldb-dap, or on a Mac without Developer Mode, the status bar says why and nothing starts. The program
   is not built first. [GEN, OSS]
   **v2:** a command that starts with a `python…` interpreter (the script is the next word, relative to
   the working directory) or with a `.py` file debugs the script through debugpy. The interpreter is the
   one named, else debugpy's own. The output goes to the Console tab, only the user's code is stepped
   ("just my code"), and the environment is passed on. Without debugpy the status bar says "Cannot debug
   Python: debugpy is not installed (pip install debugpy)"; Developer Mode is needed only for native
   programs. [GEN, STD]
3. **Tool window:** the Debug tool window (`ActivateDebugToolWindow`, ⌘5 / Alt+5; a stripe button
   appears after the first session) has:
   - a header with the configuration's name and the toolbar Resume, Step Over, Step Into, Step Out and
     Stop. The stepping buttons are enabled only while paused, Stop while a session runs.
   - the tabs "Threads & Variables" and "Console", and the state: Starting…, Running, Paused (reason),
     or the closing line.
   - on the variables tab, the call stack at the left ("function:line" and the file name) and the
     variables of the selected frame at the right ("name = value" and the type), followed by the
     frame's closure and block scopes as rows that open (v7, hand test 73). Structured values
     expand by click; their children are fetched once. The console starts with the command line,
     shown once even when the adapter prints it again (v7, hand test 95), shows the program's output and ends
     with "Process finished with exit code N" or "Debugging stopped".

   `HideActiveWindow` hides the tool window. [GEN, OSS]
4. **Execution point:** when the program pauses, the file of the top frame opens with the caret on the
   stopped line, and that line gets the `EXECUTIONPOINT_ATTRIBUTES` background. Choosing another frame
   shows its line and its locals. Editing the file or ending the session removes the mark. [GEN, OSS]
5. **Stepping:** `Resume` (⌥⌘R or F9), `StepOver` (F8), `StepInto` (F7) and `StepOut` (⇧F8) act on the
   paused thread. `Stop` (⌘F2) ends the debugged program and the adapter, or the run in the Run tool
   window when no session runs. The keys do nothing without a session, so their other bindings keep
   working. [GEN, OSS]
6. **Threads:** the adapter session lives on a thread of its own, which takes commands and reports
   updates. The window takes the updates every 50 ms, so the UI never waits for the adapter. [GEN]
7. Agents debug through `debug_start`, `debug_step`, `debug_state`, `debug_evaluate` and the other tools
   of SPEC-0054. [ADR-0008]
8. **Evaluate expression (v3):** at the top of the variables pane is an expression field ("Evaluate
   (↩)"). Enter evaluates its text in the selected frame while the program is paused. The result
   appears under the field as "expression = value", newest first, up to 20. An error from the adapter,
   or evaluating while the program is not paused, shows as "expression: reason" in red.
   `EvaluateExpression` (⌥F8 / Alt+F8) shows the window and puts the keyboard into the field. When the
   editor has the focus, the field gets the selection (within one line) or the identifier at the caret.
   Without a session the key does nothing, so its other bindings keep working. Results are cleared
   when a new session starts. [GEN, OSS]
   **(v6)** The hint "Evaluate (↩) · Watch (⇧⌘↩)" (Enter / Ctrl+Shift+Enter elsewhere) is a
   placeholder inside the field: dimmed, shown only while the field is empty, gone as soon as it has
   text, so it never overlaps what is typed. [GEN]
9. **Breakpoints kept (v3):** the breakpoints of the workspace are saved in `.idecraft/breakpoints`, one
   file per line as `path:line,line` (path relative to the workspace, 1-based lines). The file is
   written on every change and removed when there are none. Like the other machine-local state it is
   listed in the generated `.idecraft/.gitignore`, and an outdated generated ignore file is refreshed.
   An opened file gets its saved breakpoints (lines past its end are dropped). `Debug` sends all
   breakpoints of the workspace, also those of files without an open editor, so closing a tab no
   longer loses its breakpoints. [GEN]
10. **Node.js (v4):** a configuration whose command is `node script.js|mjs|cjs …` (also the temporary
   configuration of ⌃⇧R/⌃⇧D on a JavaScript file, SPEC-0097) is debugged with js-debug (SPEC-0039 v5):
   program, arguments, working directory and environment as configured, output in the console. Without
   node or js-debug the status bar says how to install it. [GEN]
11. **Watches (v5):** watch expressions are listed below the evaluated expressions as `expression =
   value` (or `expression: error`) and re-evaluated in the selected frame whenever the program pauses,
   another frame or thread is shown, or a watch is added. ⇧⌘↩ (Ctrl+Shift+Enter) in the expression field,
   `XDebugger.NewWatch` (Insert) and `Debugger.AddToWatch` (Search Everywhere; selection or identifier at
   the caret) add one; the × on a row removes it. While paused, the editor's context menu offers
   Evaluate Expression…, Add to Watches and Run to Cursor for the selection or the identifier at the
   caret (v7, hand test 21, D2). Watches stay for the next session of the window and
   show only their expression while nothing is paused. [GEN, OSS]
12. **Threads (v5):** when the program pauses, its threads are listed above the call stack, the stopped
   one marked; a click on another thread shows its call stack and locals, and stepping then acts on that
   thread. [GEN]
   **(v10)** In OBS-0014 IDECraft listed every thread of the JVM as a tree (also the system threads)
   above all frames; the reference shows one thread in a selection list and only its frames below.
   Now a selector row at the top of the frames pane shows the shown thread's name with a ▾; a click
   opens the list of all threads (the shown one marked), a click on a thread shows it and closes the
   list, so does a second click on the selector. Below the selector come only the frames of the shown
   thread. [GEN, OBS]
15. **Library frames folded (v10):** in the call stack, runs of library frames are folded into one row
   "n hidden frames" (dimmed; "1 hidden frame" for one). A click on that row shows the frames it holds
   in place, until the program stops again. A frame counts as a library frame when
   - the adapter marks it as such: the frame's presentation hint is `subtle`, or its source's is
     `deemphasize` (DAP); or
   - it has no source file in the workspace: no path, a path that is not a file (such as a `jdt://`
     address of a JDK class), or a file outside the workspace folder; or
   - its name is a generated one: it contains `$$Lambda`, `access$` or `$deserializeLambda$`, or a
     hexadecimal address `/0x`; or it starts with a JDK package (`java.`, `javax.`, `jdk.`, `sun.`,
     `com.sun.`).

   The top frame and the selected frame are never folded. Agents keep the whole stack
   (`debug_state` of SPEC-0054 is unchanged). The open question of OBS-0014 (which frames the
   reference hides, a setting) stays open; this rule is IDECraft's own. [GEN, STD, OBS]
13. **Values in the editor (v5):** while paused, the editor of the execution point shows the values of
   the shown frame's variables after the lines, `name: value` (several joined by two spaces, long values
   cut at 40 characters with "…"), in the scheme's `DEBUGGER_INLINED_VALUES` colour. Each variable
   appears once, after the last line up to the execution line (at most 50 lines above it) that names it
   as a whole word. The values go when the program resumes or ends, or when the file is edited. [GEN]
   **(v8)** The values also cover the fields of the frame's object. In OBS-0014 a Java record's method
   (`return qty * unitPriceCents;`) showed nothing: the line names only fields, while the values came
   from the frame's locals alone (there only `this`), and the field values were never fetched. Now,
   when the frame's locals hold an object named `this` or `self` that has children, its fields are
   fetched once for the shown frame (one request, after the stop is shown, so the stop is not delayed)
   and take part like locals: a field appears after the last line naming it, `qty: 3  unitPriceCents:
   25`. A local of the same name hides the field. JavaScript frames are left out, because there `this`
   is often the global object with thousands of members. The fields follow the shown frame: another
   frame or thread fetches its own. [GEN, STD, OBS]
   **(v22)** A value with line breaks, such as the source of a function that js-debug gives for a
   Node.js variable, is put on one line before it is cut, as in the Variables list (behaviour 40):
   line breaks and the white space around them become one space. The value after a line of code is
   one line of text; with a line break in it the editor could not lay it out and the application
   ended with an error (found in the harness walk of hand test 20, 2026-11-16). [GEN]
14. **The frame's object (v9):** in OBS-0014 the reference showed `this` with its object id and its
   text, and the fields the method uses (`qty`, `unitPriceCents`) at the top of the variables; IDECraft
   showed only `this = Inventory$Item@15` and its type. Now:
   - **Text of the object** (Java and Kotlin): when the shown frame has `this` with children, the
     session asks the adapter to evaluate `this.toString()` in the frame and waits at most one second.
     A text that differs from the plain value is shown after it, in quotes: `this =
     Inventory$Item@15 "Item[qty=3, unitPriceCents=25]"`, cut at 100 characters with "…". An error
     or no answer in time leaves the plain value; the session goes on, a late answer is dropped.
     Other adapters already show their object's text (Python `repr`, JavaScript's preview). [STD, OBS]
   - **Fields the method uses:** the fields of the frame's object (v8 behaviour 13) that the code of
     the paused function names as a whole word are listed at the top level after the locals, in the
     object's order, with the field icon; they open like other rows. A local of the same name hides
     the field. Set Value on such a row changes the field (its container is the object). The code of
     the function runs from its declaration — the nearest line at or above the execution line, at
     most 300 lines up, that names the function (the frame's name without class, package and
     parameters) followed by `(` — to its end: the matching closing brace, or for code without braces
     the following lines indented deeper than the declaration. Without a declaration the 50 lines
     above the execution line through it count. The file is read from disk when the program stops.
     [GEN, OBS]

16. **Current File (v11):** with no configuration in the project (otherwise the first one is chosen,
   SPEC-0087) the run widget names "Current File"; `Run`
   (⌃R) and `Debug` (⌃D) then run or debug the file in the editor like ⌃⇧R / ⌃⇧D (SPEC-0097
   behaviours 3 and 5), as the widget's buttons already did. In OBS-0025 ⌃D only said that there were
   no run configurations. [OBS]
17. **Object texts (v11):** Java values show an object's own `toString()` text after its id in every
   row, also for elements and fields (`Main$Item@33 "Item[name=pen, qty=3, cents=25]"`). java-debug
   marks such objects lazy (DAP `presentationHint.lazy`) and puts the text in their one child; the
   session now fetches that child for up to 30 lazy rows of each answer at once and shows its value in
   the row (the row still opens to the object's fields); other lazy rows (collections) keep their own
   value and children. v9's text of `this` is not added a second time when the value already carries
   it. Right after the start, before jdtls has imported the program's project, java-debug refuses to
   evaluate; the session then waits for it at most 8 seconds once, not at every stop. [OBS, STD]
18. **Smart Step Into (v11):** `SmartStepInto` (⇧F7, the ⋮ menu) while paused asks the adapter for
   the calls on the top frame's line (DAP `stepInTargets`). Only targets whose method is called in the
   line's text are kept (debugpy also lists calls of other lines of the function); when none matches,
   the adapter's list stays. With two or more, a menu under the code
   of the execution line lists them in the adapter's order (java-debug: `add(twice(sum),square(3))`,
   `twice(sum)`, `square(3)`), the first highlighted; ↑ ↓ Enter or a click steps into that call,
   Escape closes it without stepping. IDECraft steps into the line's calls one by one and back out of
   each one that is not the chosen method (by the frame's method name), at most 12 times, until it is
   entered or the line is left; the stop is then shown. This works the same with every adapter that lists
   the calls, whatever its own `stepIn` with `targetId` does. With one call or none it is a plain Step
   Into. An adapter without `supportsStepInTargetsRequest` is refused with a console line saying so
   (IMP-0022). java-debug and debugpy offer it. [STD, OSS, OBS]
19. **Reset Frame (v11):** `Debugger.PopFrame` (the ⋮ menu, a frame's context menu) runs the selected
   frame again from its start (DAP `restartFrame`); the stop that follows is shown like any other. An
   adapter without `supportsRestartFrame` (debugpy, lldb-dap) is refused with a console line. A
   frame's context menu also offers Copy Stack (`name:line (file)` per frame). [STD, OSS, OBS]
20. **Forced steps (v11):** `ForceStepOver` (⌥⇧F8) steps over the line and `ForceRunToCursor`
   (⌥⌘F9 / Ctrl+Alt+F9) runs to the caret line, both without stopping at line breakpoints on the way:
   the session takes the line breakpoints of all files away and puts them back at the next stop,
   wherever it is. Force Run to Cursor refuses a line without code like Run to Cursor. `ForceStepInto`
   (⌥⇧F7) is Step Into: the adapters' Step Into already enters every frame they can show (java-debug
   has no step filters set by IDECraft; debugpy's "just my code" is the launch's setting). [OSS, STD]
21. **The ⋮ menu (v11):** after Mute Breakpoints the toolbar has ⋮ (tooltip "More") with Force Step
   Over, Force Step Into, Smart Step Into, Run to Cursor, Force Run to Cursor, Show Execution Point,
   Evaluate Expression… and Reset Frame, with the keymap's shortcuts, enabled while paused.
   `ShowExecutionPoint` (⌥F10) selects the top frame and shows its line in the editor again. [OSS,
   OBS]
22. **Completion (v11):** while paused, typing a letter, digit, `_` or `.` in the expression field
   asks the adapter for completions (DAP `completions`, the text with the caret at its end, in the
   selected frame). Up to 12 appear in a list under the field, the first highlighted; ↑ ↓ choose,
   Tab or Enter take one (the word before the caret becomes it), Escape closes the list, a click
   takes one. While the list shows, Enter takes the completion instead of evaluating. Nothing is
   asked while setting a value or a label, or for the text just completed; answers for an older text
   are dropped. An adapter without completions offers none. [STD, OBS]
23. **Variables menu (v11):** a right click on a row selects it and offers Set Value… (F2), Copy
   Value, Copy Name, Evaluate Expression… (the row's expression into the field: the name, a field
   after its parent with a dot, an element in brackets, `items[0].name`), Add to Watches (the same
   expression) and Mark Object… / Unmark Object. [OBS, GEN]
24. **Mark Object (v11):** `Debugger.MarkObject` (the variables menu, or the action while the Debug
   window has the focus) on a row whose value has an object id (Java `Class@id`, a hexadecimal
   address `0x…`) puts the row's name into the expression field as the label, with "Mark as" before
   the field; Enter labels the object, Escape leaves. Every row whose value shows the same id then
   has `[label]` in red before its name, also after stepping and in other frames, for the window's
   sessions. Unmark Object takes the label away. Rows without an id (numbers, strings) cannot be
   marked. [OBS, GEN]
25. **Step Out of Code Block (v12):** `StepOutOfBlock` (the ⋮ menu; ⌃⌥⇧F10, given by IDECraft
   because the keymaps have none) steps over until the top frame is on a line outside the
   innermost code block around its line, the function returned, or a breakpoint stopped it; at
   most 200 steps, then a console line says it is still in the block. The block runs from the
   line of its opening brace (or the header above a brace on a line of its own) to its closing
   brace; braces in strings and `//` comments do not count. Python's block is the header ending
   with `:` above the line, indented less, through the last line indented deeper. Without a block
   it is Step Out. IDECraft's own stepping, so it works with every adapter. [GEN, OSS, OBS]
26. **Thread dump (v12):** `DumpThreads` (⋮ "Get Thread Dump") writes to the console a heading with
   the number of threads, then per thread `"name"`, its state and `(group …)` where known, its
   frames as `at name (file:line)` (up to 50), `running` for a thread that is not suspended; the
   Console tab is shown. **(v13)** A frame in a JDK class names its class file (`Reference.class`),
   not jdtls's `jdt://` address. [OSS, OBS]
27. **Resume and pause every thread (v12):** ⋮ "Resume All Threads" resumes every thread
   (java-debug `continueAll`; DAP `continue` resumes all threads for the others); ⋮ "Pause All
   Threads" pauses every thread while the program runs (java-debug `pauseAll`, else `pause`) and,
   while one thread is stopped, suspends the others too, so their frames can be read (java-debug
   `pauseOthers`; debugpy and lldb-dap already stop every thread). After the others were suspended,
   Resume resumes them all. [STD, OBS]
28. **Thread states (v12, L12):** opening the thread list reads, once per stop, each thread's
   state and group. For Java a fragment evaluated in the stopped frame asks the JVM; the list then
   shows the threads under their group's name (the stopped thread's group first) with a state
   (RUNNING for RUNNABLE, WAIT for WAITING and TIMED_WAITING, MONITOR for BLOCKED, NEW, ZOMBIE for
   TERMINATED; **v13:** SLEEPING for a timed wait whose top frame is `Thread.sleep`, as the reference
   shows it) and an icon: the stopped thread at its breakpoint, a suspended one paused, else by
   state. java-debug names threads `Thread [main]`; IDECraft shows `main`. Python has no thread
   states and debugpy stops every thread together, so its list stays as it was, without groups.
   IDECraft adds no threads of its own to the program (IMP-0345). **(v13)** Escape closes the open
   list before it moves the focus to the editor. [OBS, OSS]
29. **Force Return and Throw Exception (v12):** in the ⋮ menu, refused with a console line for every
   adapter: DAP has no request to leave a method early or to throw into the program, java-debug
   and debugpy offer none of their own, and an exception thrown by an evaluation stays inside the
   evaluation (IMP-0022: refused with a reason, not ignored). [STD]
30. **Inspect (v12, L7):** the variables menu's "Inspect…" opens a tab beside Console, "Inspect
   name", with the row's value as a tree (opened one level), evaluated again in the shown frame at
   every stop so it follows the object; up to five such tabs, each with a ×. The row's expression
   is the adapter's `evaluateName` where it gives one (java-debug `items.get(0)`, debugpy
   `items[0]`), else v11's row expression. [OBS, STD]
31. **Referring objects (v12, L7):** "Show Referring Objects…" opens such a tab with the objects that
   refer to the row's value: Python's `gc.get_referrers` without frames. java-debug offers no
   request for it (the JVM could), so it is refused with a console line. [OBS]
32. **View as (v12, L7):** the variables menu's "View as" shows the row's value as Default, Decimal,
   Hex (`0xFF`, capitals since v13), Binary (`0b101`) or Character (`'A' 65`) — computed from the shown integer, for
   every adapter — or as String (Java `String.valueOf(e)`, Python `str(e)`), Collection Elements
   (`(e).toArray()`, `list(e)`) or Map Entries (`(e).entrySet().toArray()`, `list((e).items())`),
   evaluated in the frame; the element views open to the evaluated value's children. The choice
   stays for that expression for the window's sessions; Default goes back. **(v13)** The submenu
   offers only the views that fit the value — the numeric ones for a number, String and the element
   views otherwise — and the values after the lines in the editor follow the view (`big: 0xFF`),
   as in the reference. [OBS, STD]
33. **Renderers (v12, L7):** "Renderer for This Type…" puts the type's renderer (or `this.toString()`
   / `str(self)`) into the expression field with "Show <type> as" before it; Enter saves it, an
   empty field removes it, Escape leaves. A renderer is an expression in which `this` (Java) or
   `self` (Python) is the value; rows whose type is the renderer's type, or ends with it as simple
   name, show its result instead of the adapter's value (they still open to their fields). The
   renderers are kept in `.idecraft/debug-renderers.json`; "Mute Renderers" switches them off for
   the window. Java and Python only. [OBS, GEN]
34. **Filters (v12, L7):** the variables menu's "Filter" switches Show Static Fields (Java: java-debug's
   `showStaticVariables` setting through jdtls, the frame is read again; Python: the "class
   variables" group), Show Synthetic Fields (names with `$` such as `this$0`; Python's special and
   function variables) and Show Null Values. Static fields start hidden in Java, as java-debug
   shows them, and shown in Python. [OBS, OSS]
35. **Mark colours (v12, L8):** while Mark Object asks for the label, six colour dots of the theme's
   console colours (red, yellow, green, blue, magenta, cyan) follow the field; a click chooses, red
   is the default. The label `[name]` is drawn in the chosen colour. **(v13)** A click on a dot
   leaves the keyboard in the field, so Enter still gives the mark. [OBS]
36. **Code fragments (v12, L9b):** a button at the end of the expression field, or ⇧↩, turns it into
   a code fragment of several lines: Enter starts a new line, ⌘↩ (Ctrl+Enter) evaluates; the button
   turns it back. A text with a line break is evaluated as a fragment too. java-debug evaluates the
   statements as they are and gives the last one's value (`int s = 0; for (int v :
   stock.values()) s += v; s` — the fragment the reference refused in OBS-0025, IMP-0355). For
   debugpy the lines but the last run in the `repl` context, then the last line is evaluated; an
   indented last line runs with the rest and gives no value. [STD, OBS]
37. **Hot swap (v12, L15):** saving a Java source while a Java session runs or is paused tells
   jdtls (open, change and save of the file), which builds the changed classes; when java-debug
   reports the build (its `hotcodereplace` event `BUILD_COMPLETE`, at most 60 s) a bar above the
   tabs says "Code changed: <files>" with "Reload Changed Classes" (`Hotswap`, the ⋮ menu,
   ⌃⌥⇧F9), and the console says so. Reloading (java-debug `redefineClasses`) writes "Reloaded n
   classes: …" or the reason to the console; the program runs the new code from the next call of a
   changed method. A failed build is a console line. Other adapters refuse with a console line
   (debugpy cannot reload code; restart the session). **(v13)** While the editor shows errors in a
   changed source, Reload Changed Classes refuses with a console line naming the files and their
   error counts: jdtls builds such classes with methods that throw, and loading them would break the
   running program; the reference does not reload them either. The result line stays in the
   console (IMP-0361). [OSS, OBS]
38. **Console (v12, L17):** the Console tab gets a side bar: Up the Stack Trace and Down the Stack
   Trace (also `PreviousOccurence`/`NextOccurence`, ⌥⌘↑/⌥⌘↓, while the Debug window has the
   focus) mark the previous or next line that names a stack frame — Java `at …(File.java:12)`,
   Python `File "…", line 12` — and open that file at that line (an absolute path as it is, else a
   file of that name among the open editors or in the workspace, at most six folders deep, without
   hidden and build folders); Soft-Wrap; Scroll to the End (on by default: new output scrolls
   there); Clear All — **(v13)** with the platform's arrow, soft-wrap, scroll-down and
   delete icons. Below the output an input line `>>>`: Enter runs its text in the selected
   frame with DAP context `repl` while paused — Python statements too, their printed text arriving
   as the program's output — and writes `>>> text` and the value; while running it says the
   program is not paused. [OBS, STD]
39. **Agents (v12):** `debug_resume` takes `step_out_block` and `resume_all`; `debug_pause` takes
   `all`; `debug_evaluate` takes `context` (`watch`, `repl`) and `fragment`; `debug_threads` lists
   the threads with state, group and frames; `debug_inspect` returns a value in a view with its
   children, or its referring objects; `debug_hot_swap` builds changed sources and reloads them;
   `debug_field_watch` watches a field (SPEC-0146 v3); `debug_renderers` lists and sets renderers
   (dry run first). Refusals are the window's. [ADR-0008]

40. **One row per value (v14):** a value with line breaks (a function's source) is drawn on one row of
    Variables and Inspect: line breaks and the white space around them become one space, a value too
    long for the row is cut with an ellipsis. Copying and setting the value use the full text. [GEN,
    OBS-0028 window check 2026-11-09]
41. **One after the other at the end of a line (v15):** while inline values are shown, the items at the
    end of a line are placed in this order, each after the end of the one before: text, fold placeholder,
    debug value, usage count, author. Nothing is drawn over another item. [GEN, OBS-0028 F8]

42. **JVM values (v16, OBS-0035 L11 and L12):** in Java and Kotlin sessions every answer with
    variables (locals, children, watches' children) is brought into one form before anything shows
    it. The Variables list, Inspect, the values at the end of a line in the editor and the MCP
    tools (`debug_start`, `debug_resume`, `debug_variables`, behaviour 7) therefore show the same
    text.
    - **No receiver, no row:** a variable named `this` whose value is `null`, that has no children
      and whose type is missing or the adapter's "Unknown type" is left out. kotlin-debug-adapter
      sends it for top-level functions (and other static frames). A `this` that is an object stays,
      with its fields (behaviours 13 and 14). Other `null` values stay; their "Unknown type" is not
      shown as a type. [OBS, third-party open source]
    - **Type and id:** an object reads `Type@id`: the class name without its package (`ArrayList`,
      `Map$Entry` for a nested class), `@`, the debugger's object id. kotlin-debug-adapter's
      `instance of java.util.ArrayList(id=566)` becomes `ArrayList@566`, its array
      `instance of int[3] (id=570)` becomes `int[3]@570` (the length stays in the brackets).
      java-debug writes this form already. The full type stays in the row's type text. Mark Object
      (behaviour 24) and instance filters use this id for Kotlin too. In the Variables list and
      Inspect the `@id` part is drawn in the dimmed colour. [GEN]
    - **Summary:** a collection or map is followed by ` size = N` (`ArrayList@566 size = 1`), also
      when empty. A boxed number, boolean or character of kotlin-debug-adapter (`java.lang.Integer`, `Long`, `Short`,
      `Byte`, `Double`, `Float`, `Boolean`, `Character`) shows its value alone (`300`), without type
      and id; java-debug's own text for boxed values is kept. A string shows its text in quotes, an array its length in the brackets. [GEN, OBS]
    - **Where the size comes from,** in this order: (a) the adapter's own text (java-debug's
      ` size=N`, rewritten to ` size = N`); (b) the adapter's `indexedVariables` plus
      `namedVariables` when their sum is above zero and the value is not an array; (c) for
      kotlin-debug-adapter, which gives neither, only for classes in `java.util` or
      `kotlin.collections` (and their sub-packages): the fixed size of the empty and one-element
      classes (`EmptyList`, `EmptySet`, `EmptyMap`, `Collections$Empty…` 0;
      `Collections$Singleton…` 1), else the object's fields from one `variables` request: a field
      `size`; else the length of the array in a field `a` (`Arrays$ArrayList`); else for
      `java.util.ArrayDeque` the fields `head`, `tail` and the length of `elements`; else, for a
      set kept in a map (a field `map` or `m` that is a `java.util` map), that map's `size` from one
      more request. No such field: no summary. A boxed value comes from the field `value` of the
      same kind of request. [STD, third-party open source]
    - **Nothing runs in the program:** these requests read fields; no `evaluate` is sent for a
      summary, so no method of the program (`size()`, `toString()`) is called for it. The object
      text of behaviours 14 and 17 is unchanged, and never asked for a collection's size. Classes
      outside `java.util` and `kotlin.collections` get type and id only. [GEN]
    - **Cost:** at most 30 rows of one answer get such a request, each waits at most one second,
      all of one answer together at most two; rows beyond that, and rows whose request fails or is
      late, show type and id without a summary. Children that are not shown are not asked. [GEN]
    - kotlin-debug-adapter answers `unknown` for `this.toString()`; that is no object text
      (behaviour 14) and is not shown. A value an `evaluate` returns (expression field, watches) is
      brought into the same form, with its summary. [third-party open source]

43. **Go (v18):** a configuration whose command is `go run …` or `go test …` — also the temporary one
    of "Current File" for a Go file, of a `_test.go` file and of a test function's ▶ marker
    (SPEC-0045, SPEC-0087) — is debugged with Delve (SPEC-0039 v7 behaviour 11): Debug builds and
    starts it, the Debug window shows goroutines in the thread selector, and breakpoints, stepping,
    variables, watches and evaluation work as for the other adapters. A command Delve cannot debug
    (several packages, another `go` subcommand) fails in the Debug window with the reason. Without
    Delve the status line says how to install it; on macOS without Developer Mode it says so, as for
    native programs. [SPEC-0039]
44. **Quick Evaluate Expression (v19):** while a session is paused, `QuickEvaluateExpression`
    (⌥⌘F8 in the bundled macOS keymap, Ctrl+Alt+F8 in the default one; Run › Quick Evaluate
    Expression) evaluates the selection of the focused editor, or the expression at its caret
    (as behaviour 8 takes it), in the selected frame and shows `expression = value`, or
    `expression: error`, below the caret as a hint that closes like the documentation popup.
    The Debug window's expression field, its results and its tab stay as they were, and the
    focus stays in the editor. Without an expression at the caret the status bar says "Nothing
    to evaluate at the caret"; with a session that is not paused it says "Cannot evaluate: the
    program is not paused"; without a session the action is not available. The mouse shortcut
    (⌥-click) of the keymap is not carried out. MCP: `debug_evaluate`. [OSS: id and keys of the
    bundled keymaps; GEN]
45. **Jump to Type Source (v20):** with a row of the Variables list selected and the Debug
    window focused, `XDebugger.JumpToTypeSource` (⇧F4 in the bundled macOS keymap) opens Go to
    Class with the simple name of the row's type as the debugger reports it, without package,
    type arguments or array brackets (`java.util.ArrayList<String>` → `ArrayList`); Enter opens
    the class. A primitive type says in the status bar that it has no declaration. Without a
    selected row the key goes to the next action of the chord. [GEN]

## Acceptance criteria
- **AK-1** Breakpoint lines follow inserted, removed and joined lines; the variables tree fetches
  children once and toggles; programs resolve relative to the working directory or on the `PATH`
  (unit tests of `debug_model`).
- **AK-2** In a real window (`idecraft ui --selftest`, with lldb-dap and a C compiler), on a compiled C
  program:
  - ⌘F8 sets a breakpoint on the caret line.
  - ⌃D stops there with `main` on top of the stack, `a = 21` among the locals and the execution
    point on the line.
  - F7 enters `twice` (`x = 21`, its line marked).
  - ⇧F8 returns to `main`.
  - ⌥⌘R runs to the end: exit code 0, the program's output in the console, the execution point
    cleared and the breakpoint kept.
  - ⌘5 hides the window.
- **AK-3** By hand: breakpoints by click, look of dot, lines and toolbar, variables of a struct, side by
  side with the reference.
- **AK-4 (v2)** Python plan: interpreter and script, `.py` command, native fallback, launch arguments
  (unit test `python_scripts_and_native_programs`); in a real window with debugpy, a script stops at a
  breakpoint with `number = 21`, steps into `twice` (`x = 21`) and runs to exit code 0 with its output
  (`--selftest`).

- **AK-5 (v3)** The breakpoints file round-trips, skips malformed lines and disappears when empty (unit
  test `breakpoints_file_round_trip`); in a real window, while paused in `main`, ⌥F8 and the
  expression `a * 2` show "a * 2 = 42", and after the session the file lists `dbg.c:8`
  (`--selftest`).
- **AK-6 (v3)** By hand: ⌥F8 with a selection, an invalid expression, breakpoints still there after a
  restart of the window.

- **AK-7 (v4)** Node plan and launch arguments (unit test `node_scripts`); in a real window ⌃⇧D on a
  script with a breakpoint in `twice` stops there with `y = 42` and resume runs it to its end with
  "result 42" in the console (`--selftest`; skipped without node or js-debug).
- **AK-9 (v5)** Inline values: whole words only, last line up to the execution line, cut values (unit
  test `inline_values_after_their_last_line`); in a real window, paused in `twice` of a Node.js script,
  the editor shows `x: 21` and `y: 42` after their lines, the watch `y + 1` shows `43`, the threads are
  listed, and after the end the values are gone and the watch keeps only its expression (`--selftest`).
- **AK-10 (v5)** By hand: watches across two sessions, a program with several threads (switching and
  stepping), the look of the values next to the reference.
- **AK-12 (v8)** The fields of `this`/`self` join the values, a local hides a field of the same name,
  JavaScript is left out (unit tests `inline_values_take_the_fields_of_this`,
  `object_fields_only_for_this_or_self`); in a real window, paused in the method of a Java record
  (`return qty * unitPriceCents;`), the editor shows `qty: 3  unitPriceCents: 25` after that line
  (`--selftest`, skipped without jdtls or java-debug).
- **AK-13 (v9)** The function's lines (braces, indentation, no declaration found), the fields it uses
  (whole words, locals hide fields) and the object's text (quotes, cut, same as the plain value) are
  unit tests (`method_lines_by_braces_and_indentation`, `fields_the_method_uses`,
  `object_text_after_the_value`); the tree lists the used fields after the locals with the object as
  their container and takes their new values after Set Value (`object_fields_at_the_top_level`). In
  a real window, paused in the record's method, the variables show `qty = 3` and
  `unitPriceCents = 25` at the top level and `this` with the record's text (`--selftest`, skipped
  without jdtls or java-debug).
- **AK-14 (v10)** The library-frame rule (hints, sources outside the workspace, generated names, JDK
  packages, the top frame kept) and the folded rows with their expansion are unit tests
  (`library_frames_by_hint_source_and_name`, `hidden_frames_fold_and_expand`). In a real window,
  paused in the record's method called from a stream, the frames show the thread selector, the
  frames of `Dbg`, and one "hidden frames" row for the stream pipeline that expands on click
  (`--selftest`, skipped without jdtls or java-debug).
- **AK-11 (v6)** In a real window, the empty expression field shows the hint and the field with
  `a * 2` does not (`--selftest`); by hand, typing a watch expression in a narrow Debug window shows
  no hint behind the text (hand test).
- **AK-8 (v4)** By hand: a Node.js project with a breakpoint, stepping and evaluating next to the
  reference.

- **AK-15 (v11)** ⌃D and ⌃R with "Current File" debug and run the file (by hand, OBS-0025
  Nachprüfung); the menus' items and enabled states, the Smart Step Into items' actions
  (`context_menu::tests::debug_menus`); completion words, object ids and row expressions
  (`completion_words_object_ids_and_row_expressions`); target lists, completion texts and the
  refusals without a capability (`session::tests::step_targets_completions_and_refusals`); the
  object text not doubled (`object_text_after_the_value`). In a real window with jdtls and
  java-debug (`--selftest`): ⇧F7 on `int r = add(twice(sum), square(3))` lists the three calls,
  choosing `square` stops in `square` with `x = 3`; Reset Frame stops again; the ⋮ menu lists its
  eight items; completion of `stock.ge` offers `get`; Mark Object labels `items`; a list element
  shows its record text.
- **AK-16 (v11)** By hand: next to the reference, Smart Step Into, Reset Frame, forced steps, the
  menus, completion and Mark Object in Java and Python (OBS-0025).
- **AK-17 (v12)** Blocks, thread states, views, renderers, filters, Python fragments, stack lines
  (`extras::tests`); the dump's text, filters and overrides of the tree, the adapter's expression,
  files of console lines (`debug_model::tests::dump_filters_overrides_and_files`); the menus
  (`context_menu::tests::debug_menus`); the new shortcuts (`keymap` tests). With real debugpy
  (`crates/debugger/tests/it/python_more.rs`): a fragment gives `6`, the `repl` input prints into the
  output, referring objects, the dump without states, the refusals of Force Return, Throw
  Exception, Reset Frame and hot swap, Step Out of Code Block from the loop to the line after it,
  Resume All to the end. With real jdtls and java-debug (`crates/workbench/tests/it/java_debug_more.rs`):
  the worker `WAIT` in group `main` and suspended, the dump, a fragment `3`, the String view of a
  list, Inspect, Step Out of Code Block to line 18, hot swap printing `value 2`. MCP tools
  (`crates/mcp/tests/it/protocol.rs`: tool list, `debug` right, renderers). Window self-test steps
  `java_debug_more_steps`, `python_debug_more_steps` (written, not yet run in the window).
- **AK-25 (v13)** `extras::tests::{thread_names_and_states, views_of_values}` (SLEEPING, `0xFF`),
  `context_menu` tests (views by kind), window self-test `IDECRAFT_SELFTEST_ONLY=java-debug` 35/35 and
  `debug-more` 26/26, full window self-test green; next to the reference in OBS-0025 re-check 2.
- **AK-18 (v12)** In the window (by hand or the self-test): the thread list's groups and icons,
  the Inspect tab, View as, the renderer, mark colours, the fragment button, the hot swap bar and
  the console's side bar and input line, next to the reference.

- **AK-19 (v14)** `debug_model` unit test `a_value_with_line_breaks_is_one_line`. Seen in the window on 2026-11-09 (OBS-0028).
- **AK-20 (v15)** `editor_view` unit test `items_at_the_line_end_do_not_overlap`; to be seen in the window with a paused program and a declaration with a usage count.
- **AK-21 (v16)** Unit tests `values::tests::{this_without_a_receiver_is_left_out,
  kotlin_objects_read_type_and_id, sizes_from_the_adapters_text_and_counts,
  sizes_from_fields_only_for_library_collections, boxed_values_show_the_value}`,
  `crates/debugger/tests/it/jvm_values.rs::{fields_are_read_and_nothing_is_evaluated, other_adapters_keep_their_values}` (a scripted adapter: which requests are sent, the limit of 30 rows) and
  `debug_model::tests::{the_object_id_is_the_dimmed_part, object_text_after_the_value}`. With java-debug:
  the list of `crates/workbench/tests/it/java_debug_more.rs` reads `ArrayList@<id> size = 1`. With
  kotlin-debug-adapter: SPEC-0132 AK-7. In the window: to be seen with a paused Kotlin program
  (dimmed id, value at the line end).
- **AK-22 (v17)** As SPEC-0127 AK-9 (v5).
- **AK-23 (v18)** The plan of `go run` and `go test` commands
  (`debug_plan::tests::go_commands_are_planned_for_delve`), the launch request and the session as
  SPEC-0039 AK-12 and AK-13. In the window: to be seen on the Mac (Debug on a Go file and on a
  test function's ▶ marker).
- **AK-24 (v19)** The hint's text for a value and an error and what the status bar says
  without a paused session (`debug_model::tests::quick_evaluation_reads_as_one_line`); in a
  window of the test harness the action without a session is not available and leaves the
  Debug window closed (`eighth_package_keymap.rs::quick_evaluate_needs_a_paused_session`).
  With a debugger: not checked.
- **AK-27 (v22, behaviour 13)** Unit test `debug_model::tests::an_inline_function_value_is_one_line`;
  in a window of the test harness a Node.js program paused with a function among its variables
  shows its values after the lines (`workflow_a_beta_handtests.rs::hand_test_20_debug_node`, with
  js-debug). In the window on the screen: hand test 20.
- **AK-28 (v20, behaviour 45)** Unit test `workbench_keymap_eleventh::tests::the_type_to_look_for`.

## Out of scope
Conditional breakpoints and logpoints in the UI (the session supports them, SPEC-0039), a breakpoints
dialog, expanding structured results of an evaluation and of watches, setting values, a separate Evaluate dialog with code
completion, attach to a process, building before debugging, adapters other than lldb-dap, debugpy, java-debug, js-debug and (v18) Delve, TypeScript without a build step (now SPEC-0168),
browsers.

**(v11)** Still out of scope: labels of marked objects inside expressions. **(v12)** Force Return
and Throw Exception (refused: no adapter offers them), referring objects in Java (java-debug offers
none), hot swap outside Java, thread states in Python, renderers with child expressions (the
reference can list chosen expressions as a value's children), type renderers for native and
JavaScript values.

## Known deviations from the reference
The reference builds the program before debugging and offers more adapters. IDECraft debugs native
programs through lldb-dap and Python scripts through debugpy. ⌥F8 uses the inline expression field of
the variables pane instead of a modal dialog, so the stack, the variables and earlier results stay
visible. Breakpoints are kept in a small plain-text file that people and agents can read, instead of
inside a workspace XML file. Values in the editor show each variable once, after the last line up to
the execution point that names it, rather than on every line that uses it, so the code stays readable.
In OBS-0014 the reference also listed the object's components at the record's declaration; IDECraft
shows each value once, where it is used last. The reference shows the object's id in braces before
the class name; IDECraft shows the adapter's value as it comes and adds the text after it. Which
frames count as library frames is IDECraft's own rule (behaviour 15), written down so people and
agents can predict it. **(v11)** Smart Step Into lists the calls in a menu under the line instead of
highlighting them in the code; Reset Frame stops where the adapter's `restartFrame` stops (java-debug
steps into the reset frame's call again) rather than before the call; Mark Object labels inline in
the expression field instead of a dialog with a colour; Set Value, Mark Object and completions use the
one field above the variables. The reference's thread list also shows its own helper threads in the
debugged JVM; IDECraft shows the program's threads only (IMP-0345). Compared
side by side for Java in OBS-0014. **(v12)** Inspect opens a tab of the Debug window instead of a
window of its own. A renderer's expression names the value as `this`/`self`, not its bare fields.
The thread dump goes to the console instead of a tab of its own; the console's stack lines can be
followed from there. Java's thread states come from the JVM through an evaluation in the stopped
frame (code runs in the program), only when the list or the dump is opened. Hot swap builds on
save instead of after an explicit build; the bar offers the reload, as the reference's notice does.
A code fragment the reference refused (OBS-0025 item 5) is evaluated (IMP-0355).

**(v13)** The result of hot swap stays in the console instead of a passing notification, and reloading is refused with the files and error counts while a changed source has errors (IMP-0361); the console's input line also evaluates in Java, where the reference has none (IMP-0362).

**(v16)** Collections show their size after type and id in IDECraft's own form (`ArrayList@566 size = 1`); the reference showed type, id and size too (OBS-0035 item 18). IDECraft shows no preview of the elements in the row, and for Kotlin no size of collection classes outside `java.util` and `kotlin.collections`, because it reads fields instead of calling the program's methods.
- **Jump to Type Source (v20):** the reference opens the type's declaration at once; IDECraft
  lists the classes of that name in Go to Class, where the person picks one when several have
  it.
