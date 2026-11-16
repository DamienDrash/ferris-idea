# ADR-0027: UI driver and semantic interface tree

- **Status:** accepted · 2026-11-07 · decided by the coordinator on the Project Owner's mandate
  (PO-STATUS 2026-11-07: "wenn ja entscheide du was das best practice und goldstandard dafür ist").
  Phases 1 and 2 apply now; phase 3 depends on the toolkit decision of
  [ADR-0019](0019-accessibility-path-for-gpui.md), which stays the project owner's
- **Related:** [ADR-0019](0019-accessibility-path-for-gpui.md) (accessibility path, spike PR #277),
  [ADR-0008](0008-mcp-server.md) (every function an MCP tool),
  [ADR-0012](0012-headless-core-capability-api.md) (`ui` capability domain, effects, error codes),
  [ADR-0026](0026-session-protocol-window-and-serve.md) (session protocol),
  [ADR-0025](0025-network-security-for-remote-connections.md) (rights, audit, nothing listens
  openly), [ADR-0003](0003-ui-toolkit.md) (toolkit only in `ui`, `workbench`, `app`),
  [ADR-0021](0021-idecraft-file-formats.md) (file classes), SPEC-0053 (agent rights), SPEC-0221
  (MCP over HTTP), SPEC-0275 (real clicks in the self-test), SPEC-0281 (dialog decisions),
  SPEC-0294 (tooltips), SPEC-0313 (masking), [SPEC-0319](../specifications/SPEC-0319-ui-driver.md)
  (first spec under this ADR)

## Context

IDECraft's window is checked by a self-test that runs inside the window process
(`crates/app/src/workbench.rs`, `idecraft ui --selftest`, 844 checks on 2026-11-07). How it finds
and operates the interface today:

- **Keys** (about 710 `Step::Key`) go through GPUI's `window.dispatch_keystroke`. Layout-dependent
  keys are resolved by hand (the key in the place of `[` on a German layout is looked up once).
- **Clicks** are mostly not clicks: about 600 `Step::Act` closures call `WorkbenchView` methods
  directly ("does what a mouse click on a control does"), so the control itself, its hit area and
  its enabled state are never exercised.
- **Real clicks** (24 `Step::Click`, 8 `Step::Hover`) use `hit_targets::mark(name)`: an invisible
  canvas records the painted bounds of a named control, and `native_click.rs` posts an `NSEvent` to
  the centre of those bounds. Names are ad hoc strings, there is no role, no accessible name, no
  state.
- **Checks** (about 870 `Step::Expect`/`Step::WaitFor`) read the view models through accessor
  methods. `WaitFor` retries every frame with a 16 ms `thread::sleep` for up to 60 s.

MCP clients see parts of the interface through purpose-built tools (`tool_windows`,
`dialog_decisions`, `ui_tooltips`), but nothing lets a client find "the Commit button" and press
it. Side-by-side comparisons drive the reference through Remote Robot and IDECraft through
keystrokes and the computer-use screen, with coordinates and system keys.

GPUI 0.2.2 has no accessibility tree (ADR-0019). Upstream GPUI has AccessKit; spike #277 ported
IDECraft to it at a pinned commit (33 files, all 1263 tests pass) and annotated three surfaces,
but the window measurements and the tree check from outside are still pending.

The Project Owner asked whether a robust UI driver would extend IDECraft itself in a sensible way
and left the choice of the standard to the coordinator. The answer is **yes**, for four reasons:
stable window self-tests and comparisons instead of pixels, coordinates and system keys; agents
(M11/M12) that can operate the interface reliably; accessibility, because the same semantic tree
serves screen readers; and UI tests for plugin authors.

## Decision

### 1. One semantic tree, in AccessKit's schema, is the source of truth

- Every view describes its interface **once**, through one annotation helper in `crates/workbench`
  (`semantics(id).role(…).name(…)…`): role, accessible name, description, value, states
  (enabled, focused, selected, expanded, checked, read-only, secret), the actions it answers, a
  **stable test id** and the **effects** of its actions (section 5).
- The tree uses **AccessKit's node schema** (crate `accesskit`, MIT OR Apache-2.0, toolkit-free):
  roles, properties and action requests as AccessKit defines them, mapped by AccessKit's adapters
  to NSAccessibility (macOS), UI Automation (Windows) and AT-SPI (Linux). IDECraft adds no
  test-only properties. The stable test id is AccessKit's author id, which is `AXIdentifier` on
  macOS, `AutomationId` on Windows and the accessible id on Linux — the same field platform test
  frameworks use.
- **The driver reads the tree in-process**, as the platform adapter does, not through the
  operating system's accessibility API from outside. The data are identical; in-process reading
  needs no system permission, is fast, works the same on every platform and works without a
  screen.
- A **conformance check** reads the tree from outside through the platform API (on macOS
  `scripts/ax-tree.swift` from #277) and compares role, name, id and actions with the driver's
  tree. This proves that what tests see is what screen readers get.

### 2. Automation after Playwright, WebDriver BiDi and XCUITest

- **Locators** name what a user perceives: role plus accessible name (exact or substring, case
  rule), or the test id; with text, state filters, scoping (`within`), and `nth` only when asked
  for. Locators are lazy: they are resolved again for every action and every retry. An action on a
  locator that matches more than one element fails (strict, as Playwright) and lists the matches.
- **Element ids** are AccessKit node ids, stable for the node's lifetime. A handle whose node is
  gone answers `stale`; locators never go stale.
- **Auto-waiting:** before an action the driver waits until the target is attached, visible
  (non-empty bounds after clipping), stable (same bounds in two consecutive frames), enabled,
  editable for text input, and — for pointer actions — actually receives the event (the hit test at
  the action point lands on the target or a descendant). Every wait has a deadline; on expiry the
  error names the check that failed and the element's state.
- **Events, not polling:** waits and assertions are re-evaluated when the tree changes or a frame
  is presented, never on a timer. No fixed sleep remains in the driver.
- **Actions:** `click` (count, button, modifiers), `context_click`, `hover`, `press` (keys with
  modifiers, as the keymap names them), `type` (Unicode text), `scroll`, `scroll_into_view`,
  `drag` (to an element or an offset), `focus`, `select`, `expand`/`collapse`, `set_value`, and
  the element's own AccessKit actions.
- **Assertions** retry until their deadline (web-first assertions): visible, hidden, enabled,
  disabled, focused, checked, selected, expanded, name, value, text, count, and "no element".
- **Screenshots** of one element or the window, from the window's own rendered frame, with
  masking (section 6).
- **Traces:** every action records the locator, the resolved element, each actionability check
  with its waiting time, the input events delivered, the tree difference before and after, element
  screenshots, and stderr lines, so a failed run can be understood without running it again.
- **Headless** where GPUI allows it: on GPUI's test platform (no screen) the tree, the locators,
  semantic and in-process input and the assertions work; screenshots need a renderer and answer
  `unsupported` there.

### 3. Input is synthesised inside the process, never system-wide

Three levels, all delivered only to IDECraft's own window:

| Level | What | Used for |
|---|---|---|
| **semantic** | the AccessKit action request (`Click`, `Focus`, `Expand`, `SetValue`, `ShowContextMenu`, `ScrollIntoView`, …), handled by the same code that answers a screen reader | `set_value`, `expand`, `select`, invoking without a pointer; the screen-reader path itself |
| **window** (default) | toolkit input events dispatched into the window (`PlatformInput` mouse, scroll and key events; text through the window's input handler, the path an input method commits text on) | `click`, `context_click`, `hover`, `drag`, `scroll`, `press`, `type` |
| **native** (opt-in, macOS) | `NSEvent`s posted to IDECraft's own application queue, as `native_click.rs` does today | tests of the platform layer: `rightMouseDown:`, Control-click, the system's own event translation |

- `type` commits Unicode text through the input handler, so it does not depend on the keyboard
  layout, dead keys or an input method (`@`, `ö`, `é`, `日本語`, emoji all arrive as written).
- `press` takes the key as the keymap names it (`cmd-shift-a`, `alt-f12`, `ctrl-click` is a
  `click` with modifiers); the driver resolves layout equivalents itself (on a German layout `[`
  is pressed where the system puts it), and Option combinations are delivered as a key with the
  `alt` modifier, never as the character the layout would type.
- **System-wide synthetic input (CGEvent, XTest, SendInput) is not used.** It needs the
  Accessibility permission, goes to whichever window has focus — the user's other applications
  included — depends on key codes of the layout, and stops working when the screen locks.

### 4. Interfaces: one capability domain `ui.*`, four adapters

The driver is the `ui` capability domain of ADR-0012 (`ui.snapshot`, `ui.query`, `ui.act`,
`ui.expect`, `ui.wait`, `ui.screenshot`, `ui.trace.*`, `ui.changed`), implemented once in a
toolkit-free crate `crates/uidriver` with typed inputs, stable error codes and cancellation.
Adapters only translate:

- **Rust API** for the self-test and in-process tests: `driver.role(Role::Button).name("Commit")
  .click().await?`, `expect(driver.id("project-tree")).to_be_visible().await?`.
- **MCP tools** (ADR-0008) in the window's MCP server (SPEC-0221): `ui_snapshot`, `ui_find`,
  `ui_act`, `ui_expect`, `ui_screenshot`, `ui_trace`, `ui_driver_status`, `ui_driver_stop`. The
  stdio server has no window and answers `unavailable`. `ui_tooltips` becomes a view on the tree.
- **Command line** `idecraft drive <verb> …` for scripts: a client of the window's MCP server
  through its discovery file (as `idecraft call`), plus `idecraft drive run <script>` for a UI
  script, a project file of kind `ui-script` (ADR-0021, TOML).
- **Session protocol** (ADR-0026): family `ui` with the same methods, capability `ui` in
  `session.hello`, served by the process that owns the window — for a paired device of M16 with
  the grant `ui`. The window **refuses `ui.*` from a backend** of a remote project: a host never
  operates the window (ADR-0025 T5).

### 5. Security first (AGENTS.md rule 12)

- **External clients are off by default.** Setting "Allow agents and scripts to operate the
  window" (`ui.driver.external`, default off) is changed only by the user in Settings; it is
  neither writable through `set_ide_settings` nor operable through the driver. The in-process
  self-test does not depend on it.
- **Transport:** only the window's loopback MCP server with its per-start token and origin checks
  (SPEC-0221), or the authenticated session of ADR-0025/ADR-0026. The driver opens no listener of
  its own.
- **Right `ui` per workspace** (SPEC-0053 model: grant file in the user folder, asked in the window,
  never in the project). A client of one project's route reaches only that project's windows.
- **Effects are checked, not only `ui`:** each actionable node declares the effects of its action
  (`none`, `write`, `vcs-write`, `run`, `debug`, `remote`, `ssh`, `trust`, `quit`, …, derived from
  the shared action ids). An external client needs `ui` **and** every right the effect needs, as if
  it had called the corresponding MCP tool. An actionable node without a declared effect is refused
  to external clients (`undeclared`); a test keeps the list empty.
- **Reserved for the user:** what SPEC-0281 reserves (an agent's request for a right, plugin
  installation and its rights, "Always Allow" for plugin commands), the trust question and "Trust
  Project…", the Settings pages for agent rights, MCP and the driver, keychain prompts, typing into
  secret fields, and actions that read the system clipboard (Paste) are refused to external clients
  (`reserved`). Native alerts, system dialogs and other applications are outside the tree and
  unreachable.
- **Secrets never leave:** a secret node (role `PasswordInput`, or marked secret) exposes role,
  name and whether it is empty — never its value, not in the tree, a query result, an event, a
  trace or a log. Screenshots fill the bounds of secret nodes before the image leaves the process;
  terminal text in the tree and in screenshots is masked as SPEC-0313 masks it, and shown at all
  only with `terminal-read`. Documents outside the workspace root are withheld as `read_file`
  withholds them.
- **Visible marking:** while an external client drives, the window shows it — a frame in the
  accent colour, a status-bar chip "Automated by <client>" with Stop, a short highlight of each
  target element and a pointer marker for pointer actions; screen readers hear an announcement.
  Synthesised events carry their origin, so undo, the audit log and the product can tell them from
  the user's.
- **The user wins:** physical input while an external client drives pauses it (the client gets
  `interrupted`); **Esc** pressed by the user, the Stop button, the menu item "Stop UI Automation"
  or `idecraft drive stop` is the **emergency stop**: it cancels every pending action within one
  frame, ends every external driver session, withdraws session-only grants of `ui`, and switches
  `ui.driver.external` off until the user switches it on again.
- **Audit:** every external action is logged (`.idecraft/agent-log.jsonl`, actor = client and
  session): time, action, locator, resolved role/name/id, outcome — typed text only as its length.

### 6. Order of work, and the relation to #277

The full goal — one tree that screen readers and the driver share — needs a toolkit with an
accessibility tree. **The toolkit migration of ADR-0019 is therefore a prerequisite of phase 3,
not of the driver as such.** Phases 1 and 2 start on GPUI 0.2.2:

| Phase | Content | Depends on |
|---|---|---|
| **0** | Finish the spike's pending measurements on a free screen (self-test on the spike branch, SPEC-0260 window budgets, `ax-tree` dump); the project owner decides adoption (ADR-0019, PO-STATUS question 22) | free screen, the project owner |
| **1** | `crates/uidriver` (tree, locators, actionability, assertions, masking, trace), the annotation helper — on GPUI 0.2.2 it builds the AccessKit nodes itself from the same calls (**bridge producer**, no platform adapter, so no screen reader yet) —, window-level input, `Step::Ui`/`Step::ExpectUi` in the self-test, every `hit_targets::mark` site turned into an annotation, MCP tools, CLI, security of section 5 | nothing |
| **2** | Self-test groups move to the driver one at a time (section 7); event-driven waits replace the 16 ms sleep; dialogs (SPEC-0281's 25) and tool windows annotated; side-by-side scripts use `idecraft drive` | phase 1 |
| **3** | Port `main` to upstream GPUI (the port of #277, brought up to date) or `gpui-ce`; the helper maps to GPUI's own accessibility API (role, label, `accessibility_id`, `on_a11y_action`); the driver reads GPUI's AccessKit tree updates; the bridge producer and `hit_targets` are removed; the conformance check runs in `ci-window`; VoiceOver hand tests; IMP-0005 for annotated views | phase 0 decision "adopt" |
| **4** | Session family `ui` for M16 paired devices; conformance on Linux (AT-SPI) and Windows (UIA) once those builds exist; the editor's text runs as AccessKit children | phase 3, M16 |

- The bridge producer uses the same annotation calls and the same schema, so no view is
  annotated twice and the tree tests see does not drift from the tree a screen reader will see.
  It exists only until phase 3 and has a removal criterion (section 7).
- If the project owner declines both upstream GPUI and `gpui-ce`, phases 1 and 2 stand on their own; the
  bridge producer then stays, screen readers keep seeing only native alerts, and this ADR records
  that as the one exception to "the platform tree is the source".

### 7. Migration of the window self-test

1. **Executor first:** `Step::Click` and `Step::Hover` are re-implemented on the driver with the
   mark names as test ids (no change of the 32 call sites' meaning, no coordinates in the test).
2. **Groups move one at a time**, smallest risk first: `mouse` (SPEC-0275), tooltips (SPEC-0294),
   `breakpoints`, `containers`, then dialogs, tool windows, editor, debugger, Git. A group is moved
   when it uses no `Step::Click` coordinates, no `Step::Act` that stands in for a click, and no
   fixed sleep.
3. **Ratchet:** a unit test counts `Step::Act` stand-ins and `hit_targets::mark` sites; the counts
   may only fall. `Step::Key` stays where the keymap itself is under test.
4. **End state:** `hit_targets.rs` and the `Step::Act` stand-ins are gone; `native_click.rs`
   remains only behind the driver's native level.

## Consequences

- Every new or touched view adds semantics; the annotation is part of a view's definition of done.
  Missing names or effects are found by tests, not by users.
- One new toolkit-free crate (`crates/uidriver`) and one dependency (`accesskit`, the version the
  upstream pin uses, so phase 3 does not bring a second one).
- The self-test becomes slower to break when layouts change and faster to diagnose when it does.
- Agents get a reliable, permissioned way to operate the window; nothing in it is reachable unless
  the user switches it on and grants `ui`.
- ADR-0025 section 4 gains the device grant `ui` (for M16); IMP-0382 records the reference
  platform's UI test server as a weakness not to copy.
- Cost when nobody listens: none — the tree is built only while the driver, a screen reader or the
  self-test is active (budget in SPEC-0319).

## Alternatives considered

- **A tree only for tests** (extend `hit_targets` with roles and names) — rejected as the end
  state: it drifts from what screen readers see, duplicates annotation work after the toolkit
  switch, and tests would prove nothing about accessibility. Its only advantage (available today)
  is kept by the bridge producer, which uses the real schema and the same annotations.
- **The OS accessibility API from outside as the driver** (XCUITest-like, Appium Mac2, AT-SPI
  clients) — rejected as the primary path: needs the Accessibility permission for the driving
  process, inter-process calls per attribute are slow, every platform differs, and nothing works
  headless. Kept as the conformance check.
- **Pixels, OCR or computer use** — rejected: layout-, theme- and resolution-dependent, needs the
  screen, cannot see state, and masks nothing.
- **System-wide synthetic input (CGEvent/XTest/SendInput)** — rejected (section 3).
- **Native `NSEvent`s for every action** — rejected as the default: macOS only, needs a real
  window, and key events depend on the layout; kept as the opt-in native level.
- **A separate driver protocol or port** (WebDriver HTTP server, Remote Robot style) — rejected: a
  second listener, a second authentication, a second rights model. The reference platform's UI test
  server listens without authentication and runs scripts inside the IDE (IMP-0382).
