# SPEC-0319: UI driver and semantic interface tree

- **Version:** 6 · **Date:** 2026-11-16 (v6: the question the toolkit draws where the platform has no alert — Linux — is in the tree, reserved for the user, and while one is open in a window no input of an external client reaches that window, behaviour 35, SPEC-0281 v4 behaviour 9); 2026-11-15 (v5: the native level on Linux delivers pointer events in process into the window's own input handler, never through the X server or the compositor, behaviour 20c, UISEC-9; decision on open question 40 of PO-STATUS); 2026-11-08 (v4: behaviour 37b, sessions of clients that went away; v1 written before the implementation; v2 records what
  phase 1 on GPUI 0.2.2 does differently, section "Phase 1 on GPUI 0.2.2 (v2)"; v3: the window
  check of phase 1, section "Window check of phase 1 (v3)"; phase 3 waits for the toolkit
  decision of ADR-0019)
- **Sources:** STD (AccessKit node schema, roles and action requests; macOS NSAccessibility with
  `AXIdentifier`, Windows UI Automation with `AutomationId`, AT-SPI; W3C WebDriver BiDi:
  locators by accessibility role and name, input actions, events instead of polling; Model Context
  Protocol), GEN (Playwright: role and accessible-name locators, strictness, auto-waiting with
  actionability checks, web-first assertions, traces; XCUITest: query by element type and
  identifier, waiting for existence; Testing Library: query the way users perceive the page;
  computer-use agents: user input takes over, Esc stops), OSS (JetBrains Remote Robot, Apache-2.0,
  as the reference platform's UI test tool: HTTP endpoint on a local port, XPath over the Swing
  component tree, script execution inside the IDE — used for side-by-side comparisons,
  RESEARCH_BOUNDARIES section 3), ADR-0027, ADR-0019, ADR-0008, ADR-0012, ADR-0025, ADR-0026, SPEC-0053,
  SPEC-0221, SPEC-0260, SPEC-0275, SPEC-0281, SPEC-0294, SPEC-0313
- **Feature matrix rows:** F-084 (MCP for agents); accessibility of IMP-0005
- **Roadmap:** M6.16

## Purpose
People, agents, scripts and the self-test find the controls of the IDECraft window by what they
are and what they are called — "the button Commit", "the tree item `main.rs` in the Project
tree" — and operate them reliably, without coordinates, fixed sleeps or system-wide keystrokes.
The same semantic tree that the driver reads is the one a screen reader reads. Nothing outside
IDECraft can use it unless the user switches it on and grants the right.

## Behaviour

### The semantic tree
1. **Annotation:** a view describes each meaningful element once through the helper
   `semantics(id)` in `crates/workbench`: role, accessible name, optional description and value,
   states (enabled, focused, selected, expanded, checked, read-only, secret, busy), the actions it
   answers, a stable test id and the effects of its actions (behaviour 31). On GPUI 0.2.2 the
   helper builds AccessKit nodes itself (bridge producer, phase 1); after the toolkit switch it
   calls GPUI's accessibility API (phase 3). Call sites do not change between the phases.
   [ADR-0027 sections 1, 6]
2. **Schema:** roles, properties and actions are AccessKit's. Text inputs that hold secrets have
   role `PasswordInput`; other nodes that carry a secret are marked secret. Containers that only
   lay out children are not in the tree unless they have a name. [STD]
3. **Test ids** are AccessKit author ids, unique within a window, written in kebab case
   (`project-tree`, `commit-button`, `tab-0`, `gutter-breakpoint-12`); every name used today by
   `hit_targets::mark` becomes the test id of the same element. Ids of repeated elements end with
   a stable key of their model (path, line, index of the model, not the painted row). [GEN]
4. **Element ids:** each node has an id stable for its lifetime. A handle to a removed node
   answers `stale`; locators resolve again on every use. [STD]
5. **Bounds:** each node has its bounds in window coordinates, clipped by the scroll areas and
   windows around it, and `visible: false` when nothing of it is shown. [GEN]
6. **Large content:** the editor is a multi-line text input whose value is the visible lines, with
   caret and selection; the whole document is read with `read_file` (document registry,
   SPEC-0220). Tables and trees expose their visible rows plus row and column counts. [GEN]
7. **Cost:** the tree is built only while a consumer is active (the driver, an external client, a
   screen reader after phase 3, the self-test). Without one, nothing is built. [ADR-0027]

### Locators
8. **Kinds:** `role` with `name` (exact by default, `contains`, or a regular expression; case
   sensitive unless `ignore_case`), `id` (test id), `text` (visible text of a node or its
   descendants), `label` (the element named by a label), plus filters `enabled`, `checked`,
   `selected`, `expanded`, `focused`, `has` (a descendant matching another locator), `within`
   (scope to the match of another locator, chainable) and `nth` (0-based, only when asked for).
   [GEN, STD]
9. **Text form** for the command line and scripts: `role=button[name="Commit"]`,
   `id=project-tree >> role=treeitem[name="main.rs"]`, `text="No problems"`; `>>` chains scopes,
   `name~="…"` matches a substring, `name=/…/` a regular expression, `[enabled]`, `[checked=false]`
   and `[nth=2]` are filters.
   The JSON form is the same structure as an object. Both forms parse into the same type; an
   unparsable locator answers `invalid` with the position of the error. [GEN]
10. **Strictness:** an action or a single-element assertion on a locator that matches more than one
    element fails with `ambiguous` and lists up to 10 matches with role, name, id and bounds. A
    locator that matches nothing waits (behaviour 12) and then fails with `not_found`, naming the
    closest matches by role and name. [GEN]
11. **Scope:** a locator acts on one window: the key project window, or the window named by its
    project route for MCP clients (behaviour 29). Popups, menus and dialogs of that window are part
    of its tree. [GEN]

### Waiting
12. **Auto-waiting:** before an action the driver waits until the target is attached, visible,
    stable (same bounds in two consecutive presented frames), enabled, editable (for `type` and
    `set_value`), and for pointer actions receives the event (the hit test at the action point
    lands on the target or a descendant, not on an overlay). `force: true` skips the checks except
    attached (for tests of disabled states). [GEN]
13. **Events, not timers:** waits and assertions are re-evaluated when the tree changes or a frame
    is presented. The driver asks for a frame when it waits, so an occluded or unfocused window
    still advances. No fixed sleep exists in the driver. [STD, GEN]
14. **Deadlines:** every wait has a deadline (default 10 s, per call `timeout_ms`, the self-test's
    long steps up to 180 s). On expiry the error `deadline_exceeded` names the check that failed
    last, the element's role, name, states and bounds, and how long each check waited.
    [ADR-0012 rule 2]
15. **Cancellation:** every call takes a cancellation handle; `$/cancel` (session protocol), MCP
    cancellation and the emergency stop (behaviour 37) end it with `cancelled` within one frame.
    [ADR-0026]

### Actions
16. **Pointer:** `click` (button left/right/middle, `count` 1–3, modifiers), `context_click`
    (secondary button; with `native`, also as Control-click), `hover` (pointer rests until a
    tooltip may appear), `drag` (to another locator or an offset, with intermediate moves) and
    `scroll` (delta in lines or pixels, or `scroll_into_view`). The action point is the centre of
    the visible part of the element, or `position` relative to its top left. [GEN]
17. **Keys:** `press` takes keys as the keymap writes them (`cmd-shift-a`, `alt-f12`,
    `ctrl-alt-left`, a sequence `cmd-k cmd-s`); the driver resolves layout equivalents itself
    (the key in the place of `[` on the current layout) and delivers Option/Alt combinations as a
    key with the modifier, never as the character the layout would type. [GEN]
18. **Text:** `type` commits Unicode text through the window's input handler (the path an input
    method commits text on), independent of the keyboard layout, dead keys and input methods;
    `clear: true` selects and deletes the existing value first. `set_value` sets the value through
    the AccessKit action where the element supports it. [STD]
19. **Semantic actions:** `focus`, `select`, `expand`, `collapse`, `invoke` (AccessKit `Click`
    without a pointer), `show_context_menu`, `increment`, `decrement` and any custom action the
    element lists, handled by the same code that answers a screen reader. [STD]
20. **Delivery levels:** `window` (default) dispatches toolkit input events into IDECraft's
    window; `semantic` sends the AccessKit action request; `native` (opt-in) posts `NSEvent`s to
    IDECraft's own application queue on macOS (SPEC-0275 behaviour 12) and hands the toolkit's
    pointer events to the window's own input handler on Linux (behaviour 20c). No level sends
    events to the system, to the display server or to another application. [ADR-0027 section 3]
21. **Result:** an action answers once its input was delivered and the next frame was presented,
    with the resolved element (id, role, name, bounds), the waiting time per check and the
    focused element afterwards. [GEN]

### Assertions and queries
22. **`ui_expect`** checks one matcher and retries until its deadline: `visible`, `hidden`,
    `enabled`, `disabled`, `focused`, `checked`, `selected`, `expanded`, `name`, `value`, `text`
    (exact, contains or regular expression), `count` (exactly, at least, at most), `absent`.
    `not: true` inverts. The failure states what was expected and what the element showed at the
    deadline. [GEN]
23. **`ui_find`** returns the current matches without waiting (or with `wait: true`, after the
    first match appears): id, role, name, value (masked, behaviour 33), states, bounds, test id,
    actions and declared effects. [GEN]
24. **`ui_snapshot`** returns the tree of a window or of the subtree of a locator, limited by
    `depth` and `max_nodes` (default 2000; `truncated` says what was left out), as JSON or as an
    indented text outline (`role "name" #id [states]`) for agents. [GEN]

### Screenshots and traces
25. **Screenshots** of the window or of one element come from the window's own rendered frame
    (PNG, device pixels, with the scale factor). Before the image leaves the process, the bounds
    of secret nodes and of masked terminal text are filled (behaviour 33). Where the toolkit gives
    no read-back of the frame, or on the headless test platform, `ui_screenshot` answers
    `unsupported`; the screen-recording permission is never requested. [ADR-0027]
26. **Traces:** `ui_trace start` records, per action: time, actor, locator, resolved element,
    every actionability check with its waiting time, the delivered input events, the tree
    difference before and after, an element screenshot when available, and stderr lines of the
    window process. `ui_trace stop` writes the trace as state files (ADR-0021: JSON Lines plus
    PNGs) in `ui-traces/<time>/` in the user state folder (folder 0700, files 0600), never into the
    project unless a path inside the workspace is given. Traces are masked like everything else
    and hold typed text only as its length. The self-test writes a trace of a failed step when
    `IDECRAFT_SELFTEST_TRACE` is set. [GEN]

### Events
27. **`ui.changed`** (session protocol and MCP progress notifications of a subscription) reports
    tree changes coalesced per frame: added, removed and changed nodes with the changed
    properties; **`ui.event`** reports focus changes, windows, dialogs and popups opened and
    closed, and announcements (AccessKit live regions). Values of secret nodes never appear.
    [STD]

### Interfaces
28. **Rust API** (`crates/uidriver`, toolkit-free; host side in `crates/app`): `Driver::role`,
    `id`, `text`, `label` return a `Locator` with the filters and actions above; `expect(locator)`
    returns the assertions; all are `async` on the window's foreground executor and return
    `Result<_, UiError>` with the stable codes of behaviour 40. The self-test gets
    `Step::Ui(locator, action)` and `Step::ExpectUi(locator, matcher)`. [ADR-0012]
29. **MCP tools** of the window's MCP server (SPEC-0221), each taking an optional `window`
    (default: the key window of the route's project):
    - `ui_snapshot` — behaviour 24.
    - `ui_find` — behaviour 23.
    - `ui_act` — `locator` or `element`, `action` (behaviours 16–19), its arguments, `level`,
      `timeout_ms`, `force`; behaviour 21.
    - `ui_expect` — behaviour 22.
    - `ui_screenshot` — `locator` (optional), behaviour 25; the image as MCP image content.
    - `ui_trace` — `start`, `stop` (with an optional path), `status`.
    - `ui_driver_status` — whether external driving is on, which clients drive which windows,
      whether a pause or stop is in effect; needs no right.
    - `ui_driver_stop` — the emergency stop of behaviour 37 for every client; needs no right.
    The stdio server (`idecraft mcp`) has no window and answers `unavailable` naming the window's
    HTTP server. `ui_tooltips` (SPEC-0294) keeps its output and reads it from the tree. [ADR-0008]
30. **Command line:** `idecraft drive [--project <folder>] <verb> …` with the verbs `snapshot`,
    `find`, `click`, `context-click`, `hover`, `press`, `type`, `scroll`, `drag`, `expect`,
    `screenshot <file>`, `trace start|stop`, `status`, `stop`, and `run <script>`. It reaches the
    running window through the discovery file of SPEC-0221 (no new listener), prints results as
    text or with `--json` as JSON, and exits 0 on success, 1 on a failed assertion or action, 2 on
    a usage error and 3 when no window or no driver is available. A **UI script** is a project file
    of kind `ui-script` (ADR-0021: TOML with the `[idecraft]` envelope) with a list of steps
    (`locator`, `action` or `expect`, arguments); it runs only through `idecraft drive run`, never
    by opening or importing it. `idecraft drive stop` needs no right. [ADR-0021]
31. **Session protocol:** family `ui` with `ui.snapshot`, `ui.find`, `ui.act`, `ui.expect`,
    `ui.screenshot`, `ui.trace.start`, `ui.trace.stop`, `ui.subscribe` → `ui.changed`, `ui.event`;
    capability `ui` in `session.hello`. Served by the process that owns the window, for paired
    devices with the grant `ui` (M16, ADR-0025 section 4). A window refuses every `ui.*` request
    that arrives from the backend of a remote project with `denied`. [ADR-0026, ADR-0025 T5]

### Security
32. **Off by default:** setting "Allow agents and scripts to operate the window"
    (`ui.driver.external`, Settings → Tools → UI Automation) is off. While off, every external
    `ui_*` tool except `ui_driver_status` and `ui_driver_stop`, every `idecraft drive` verb except
    `status` and `stop`, and every `ui.*` session request answer `unavailable` with the hint where
    to switch it on. The setting is not writable through `set_ide_settings` or any other MCP tool,
    and its Settings page is reserved (behaviour 35). The in-process self-test does not depend on
    it. [ADR-0027 section 5]
33. **Secrets:** a secret node exposes role, name, `secret: true` and `empty: true|false`; its
    value is never in a snapshot, a query result, an event, a trace, a log or an error message, and
    its bounds are filled in every screenshot (with a margin of 2 logical pixels). Terminal text is
    in the tree and in screenshots only for clients with `terminal-read` and then masked as
    SPEC-0313 behaviour 6 masks it; without the right the terminal's text node has no value and its
    area is filled. Editors showing a file outside the workspace root expose no value to external
    clients. [SPEC-0313, ADR-0007]
34. **Right `ui`:** external clients need the workspace right `ui` ("operate the IDECraft window:
    read its interface, click, type and press keys in it"), granted as SPEC-0053 describes (grant
    file in the user folder; a client of the window's HTTP server is asked in the window). Every
    call is checked once; a missing right answers `not_granted` naming `ui`. A client of one
    project's route reaches only windows of that project. [SPEC-0053]
35. **Effects:** every actionable node declares the effects of its action, derived from the shared
    action ids (`none`, `write`, `vcs-write`, `run`, `debug`, `remote`, `ssh`, `http`, `trust`,
    `quit`, `ports`, …). For an external client the driver checks each effect's right before
    delivering input, as the corresponding MCP tool would; a missing one answers `not_granted`
    naming it, and nothing is delivered. An actionable node without a declared effect answers
    `undeclared`. **Reserved** nodes answer `reserved` to external clients: the answers SPEC-0281
    reserves for the user, the trust question and "Trust Project…", the Settings pages for agent
    rights, MCP and UI automation, keychain prompts, typing into secret nodes, and every action
    that reads the system clipboard (Paste and its variants). Native alerts, system dialogs and
    other applications are not in the tree. **(v6)** Where the toolkit draws the alert itself
    (Linux, SPEC-0281 v4 behaviour 9), the question and its answers are in the tree, modal and
    reserved (`decision`); while such a question is open in a window, every action of an external
    client on that window answers `reserved` and delivers nothing — checked when the call comes,
    when its input is planned and again before each key is delivered, so a question that opens
    meanwhile, also one the client's own call caused, is never answered by it. [ADR-0027 section
    5, rule 12]
36. **Visible marking:** while an external client drives a window, that window shows a 2-pixel
    frame in the accent colour, a status-bar chip "Automated by <client name>" with a Stop button,
    a highlight of each target element for 300 ms before input is delivered, and a pointer marker
    at the action point of pointer actions; the start is announced to screen readers. The marking
    is drawn as an overlay and changes no layout. Synthesised events carry their origin (`user`,
    `selftest`, or `driver:<client>`), visible to undo grouping and the audit log. [ADR-0027]
37. **The user wins:** physical keyboard or pointer input in a window that an external client
    drives pauses that client for 3 s after the last input (pending and new actions answer
    `interrupted`). **Emergency stop:** Esc pressed by the user while the marking is shown, the
    Stop button, the menu item "Stop UI Automation" (Tools), `ui_driver_stop` or
    `idecraft drive stop` cancels every pending action within one frame, ends every external driver
    session, withdraws grants of `ui` given for this session only, switches `ui.driver.external`
    off until the user switches it on again, and is logged. [GEN]
38. **Audit:** every external action, assertion, screenshot and trace start is written to
    `.idecraft/agent-log.jsonl` with time, actor (client name and session), tool, locator, resolved
    role, name and test id, effects, outcome and duration; typed text only as its length, never a
    value. [SPEC-0053, ADR-0025 section 4]
39. **Transport:** the driver opens no listener. External clients reach it only through the
    window's MCP server on loopback with its token and origin checks (SPEC-0221) or through an
    authenticated session (ADR-0025, ADR-0026). [ADR-0025 section 5]

### Errors and budgets
40. **Error codes** (`data.code` in the session protocol, the code prefix of MCP errors):
    `not_found`, `ambiguous`, `stale`, `not_visible`, `not_enabled`, `not_editable`,
    `obscured` (the hit test lands on another element, which is named), `deadline_exceeded`,
    `cancelled`, `interrupted`, `not_granted`, `reserved`, `undeclared`, `unavailable`,
    `unsupported`, `invalid`, `denied`, `internal`. Each error carries a hint. [ADR-0012, ADR-0026]
41. **Budgets** (SPEC-0260 style, measured by the bench with the workbench fixture): with no
    consumer the tree costs nothing measurable (typing to frame p95 unchanged within 0.1 ms); with
    the driver active, building and diffing the tree adds at most 1 ms p95 per frame with 5 000
    nodes, typing to frame stays within SPEC-0260's budget, and the time from a tree change to a
    waiting assertion resolving is at most one frame. [SPEC-0260]

### Migration of the window self-test
42. **Executor:** `Step::Click` and `Step::Hover` are implemented on the driver, addressing their
    targets by the test ids that replace the mark names; `Step::Ui` and `Step::ExpectUi` are added.
    `WaitFor`/`WaitLong` wait on tree and frame events instead of the 16 ms sleep. [ADR-0027 section 7]
43. **Groups move one at a time** (`mouse`, tooltips, `breakpoints`, `containers`, dialogs, tool
    windows, editor, debugger, Git); a group counts as moved when its steps use no coordinates, no
    `Step::Act` that stands in for a click and no fixed sleep, and it passes three runs in a row.
    `Step::Key` stays where the keymap is under test; `Step::Act` stays for set-up that is not a
    user action (writing a fixture file, starting a server). [ADR-0027 section 7]
44. **Ratchet:** a unit test counts the `Step::Act` stand-ins (marked as such) and the
    `hit_targets::mark` sites in the workspace and fails if either count rises above the number
    recorded in the test. [GEN]
45. **Conformance (phase 3):** `ci-window` dumps the platform accessibility tree of the self-test
    window (`scripts/ax-tree.swift` on macOS) and compares role, name, test id and actions of every
    node with a test id against the driver's tree; a difference fails the run. [ADR-0027 section 1]

## Acceptance criteria
- **AK-1** *Locators:* given a fixture tree with two buttons "Commit" and "Commit and Push", when
  `role=button[name="Commit"]` is resolved, then it matches one (exact name) and
  `role=button[name~="Commit"]` (contains) fails an action with `ambiguous` listing both; the text
  and JSON forms parse to the same locator; an invalid locator answers `invalid` with the position.
- **AK-2** *Selftest group `mouse` without coordinates:* `IDECRAFT_SELFTEST_ONLY=mouse` passes with
  every click, context click and hover going through `Step::Ui` by role and name or test id; the
  group's source contains no coordinates, no `hit_targets::get` and no `std::thread::sleep`.
- **AK-3** *No fixed sleeps:* the driver crate and the self-test executor contain no
  `thread::sleep` and no timer-based retry (workspace scan test); a waiting assertion on an element
  that appears after a 300 ms delay resolves within one frame of its appearance (measured in an
  in-process test), and one that never appears fails at its deadline with the last failed check.
- **AK-4** *Right click and Control-click:* in the Project tree, `context_click` on a tree item
  opens its context menu at level `window`, and at level `native` both the secondary button and
  Control+primary button open it (SPEC-0275); the menu's items are found by role `menuitem` and
  name.
- **AK-5** *Option and modifiers:* on a German and on a US layout, `press("alt-f12")` opens the
  terminal and `press("cmd-[")` (macOS) goes back (SPEC-0071 v4 behaviour 6) without typing a
  character into the editor; `click` with `alt` on a stripe button shows the window in its other
  half (SPEC-0061 v10).
- **AK-6** *Unicode:* `type` of `"@ö é ß 日本語 🦀"` into the editor and into the Search Everywhere
  field arrives exactly as written on a German and a US layout, with no dead-key or modifier
  side effects.
- **AK-7** *Actionability:* clicking a disabled button waits and fails with `not_enabled`; clicking
  a button covered by a popup fails with `obscured` naming the popup; a button that moves for two
  frames is clicked only after it stops; `force: true` clicks the disabled button and the test
  sees that nothing happened.
- **AK-8** *MCP and CLI:* with the setting on and `ui` granted, a client of the window's HTTP
  server opens the Commit window with `ui_act` (`press` of its shortcut), finds the Commit button
  with `ui_find`, asserts it disabled with `ui_expect`, and `idecraft drive find
  'role=button[name="Commit"]' --json` prints the same element; the stdio server answers
  `unavailable`.
- **AK-9** *Screenshots and traces:* `ui_screenshot` of the connection dialog with a filled
  password field returns a PNG in which the field's bounds are filled; a trace of three actions
  contains every check with its waiting time and the tree differences, and no typed text.
- **AK-10** *Headless:* where GPUI's test platform is available, the locator, waiting, semantic and
  window-level action tests run without a screen in `cargo test`; screenshot tests are skipped
  there with `unsupported`.
- **AK-11** *Migration ratchet:* the ratchet test (behaviour 44) exists with the counts of the
  first implementation and fails on an added `Step::Act` stand-in or `hit_targets::mark` site.
- **AK-12** *Budgets:* behaviour 41 holds in the window bench.
- **AK-13** *Conformance (phase 3):* the `ax-tree` dump of the self-test window and the driver's
  tree agree for every node with a test id (role, name, id, actions); the safe-mode banner, the
  drawn Delete confirmation and the Project tree of #277 are among them.

### Security tests (mandatory)
| ID | Test | Expected |
|---|---|---|
| UISEC-1 | **Off by default:** fresh settings; every `ui_*` tool, every `idecraft drive` verb and every `ui.*` request | `unavailable` with the hint, except `ui_driver_status`/`ui_driver_stop`/`status`/`stop`; nothing delivered, nothing in the tree built for the client |
| UISEC-2 | **Setting not writable by agents:** `set_ide_settings` with `ui.driver.external`; `ui_act` on the Settings page of UI automation and on agent rights | refused (`invalid` naming the key; `reserved`); setting unchanged |
| UISEC-3 | **Missing right:** setting on, no `ui` grant; then `ui` granted but not `vcs-write`, and `ui_act` on Commit | `not_granted` naming `ui`, then naming `vcs-write`; no input delivered; both logged |
| UISEC-4 | **Reserved:** external `ui_act` on the agent-right question's buttons, "Trust Project…", plugin installation, "Always Allow", a keychain prompt, `type` into a password field, `press("cmd-v")` and the Paste menu item | `reserved`; nothing changes; logged |
| UISEC-5 | **Undeclared effect:** a test view with an actionable node without declared effect | external `ui_act` answers `undeclared`; the workspace test listing undeclared actionable nodes of the real views is empty |
| UISEC-6 | **Secrets never leave:** a connection dialog with a known password, a run configuration with a secret environment value and a terminal printing a token; scan `ui_snapshot`, `ui_find`, `ui.changed`, `ui.event`, errors, traces, `agent-log.jsonl` and stderr; decode every screenshot | the known secrets appear nowhere; secret bounds filled in the PNG; terminal text absent without `terminal-read`, masked with it |
| UISEC-7 | **Project isolation:** two project windows; a client of project A's route addresses project B's window | `not_found` (B's windows are not visible to A's route); nothing delivered |
| UISEC-8 | **Host cannot drive:** the backend of a remote project sends `ui.snapshot` and `ui.act` over its session | `denied`; logged; the window unchanged |
| UISEC-9 | **No listener, no system input:** list the listening sockets of the window process with the driver on and off; intercept event posting; on Linux, an observer client of the same X display watches the raw input of every device while the native level clicks | no new socket; no `CGEventPost`/`CGEventCreate*`, XTest, `XSendEvent`, virtual-pointer or uinput call in the workspace (scan test); native events only to IDECraft's own application (macOS) or its own window's input handler (Linux, behaviour 20c, called in `native_click.rs` only); the observer sees no input and the display's pointer does not move |
| UISEC-10 | **Emergency stop:** while a client waits on a 10 s assertion and another drags: physical Esc, Stop, menu item, `ui_driver_stop`, `idecraft drive stop` (one per run) | pending calls answer `cancelled` within one frame; sessions ended; session-only `ui` grants gone; setting off; logged; a new call answers `unavailable` |
| UISEC-11 | **User input wins:** physical key and pointer input during an external action sequence | actions answer `interrupted` until 3 s after the last input; no input of the client is mixed into the user's typing |
| UISEC-12 | **Marking:** an external client drives | frame, chip with the client's name and Stop, target highlight and pointer marker present (read through the tree and the painted frame); no layout change (bounds of all nodes equal with and without marking) |
| UISEC-13 | **Audit without content:** `type` of a known text | log entry with length only; the text appears in no log or trace |
| UISEC-14 (v6) | **Drawn question open:** external `press("enter")`, `press("tab space")` and a click on an answer while the toolkit's question (Linux, SPEC-0281 v4 behaviour 9) is in the tree; a question that appears while a press waits for its highlight | `reserved`; no key delivered, the question stays open; an in-process actor (the self-test) is not refused |

## Out of scope
- Driving native alerts, system dialogs (open/save panels, keychain) and other applications.
- Pixel comparison of screenshots (visual regression); screenshots are diagnostic.
- Record-and-replay of user sessions into scripts (a later spec may add a recorder on top of
  the event stream).
- Driving the reference IDE; side-by-side scripts keep using Remote Robot there, with the same
  locator vocabulary where the reference exposes accessible names.
- A published SDK for plugin authors (M11.4, after GO-4); the Rust API and `idecraft drive` are
  the interfaces until then.

## Phase 1 on GPUI 0.2.2 (v2)
Phase 1 is implemented without a screen. Where GPUI 0.2.2 or the state of the views does not allow
behaviour v1 as written, the safer variant applies until phase 3; each item names what lifts it.

- **1a — Hierarchy of the bridge producer.** GPUI 0.2.2 tells an element nothing about its
  parents, so the bridge producer derives them from the painted bounds: the parent of a node is
  the smallest node whose bounds contain it (of equal bounds the earlier one); a node marked as
  overlay (menus, dialogs, tooltips) is never the parent of a node painted before it. "Receives
  the event" (behaviour 12) is answered from the annotated nodes only: a later-painted node at the
  action point that is neither an ancestor nor a descendant obscures the target. An overlay that is
  not annotated is not seen. Lifted in phase 3 (GPUI's own tree).
- **3a — Marks as nodes.** Every `hit_targets::mark` site is, while the tree is built, a node of
  role `GenericContainer` with the mark's name as test id — no name, no actions, no effects. The
  self-test addresses it; external clients can read it but every action on it answers
  `undeclared` until its view annotates it (`.semantics(…)` with role, name and effects, phase 2).
  So the workspace list of undeclared actionable nodes of the real views (UISEC-5) is empty.
- **16a — Pointer actions for external clients.** `Window::dispatch_event` of GPUI 0.2.2 answers a
  type private to the toolkit, so no code outside it can dispatch pointer events into a window.
  At the window level, pointer actions (`click`, `context_click`, `hover`, `drag`, `scroll`)
  answer `unsupported` before anything is delivered; keys (`press`) and text (`type`) go through
  the public `dispatch_keystroke`, text through the input handler. The native level stays
  reserved to the in-process self-test (20a). Lifted in phase 3 or by a small toolkit
  contribution.
- **19a — Semantic actions.** GPUI 0.2.2 has no accessibility actions; the semantic level answers
  `unsupported` (except `scroll_into_view` of a node that is already shown, which needs nothing).
  Lifted in phase 3 (`on_a11y_action`).
- **20a — The native level only in process.** Native events posted to IDECraft's own queue cannot
  be told from the user's own input when they arrive, so the user could not take over
  (behaviour 37): external clients get `unsupported` for `level: native`. The self-test's clicks
  use it, as before (SPEC-0275 behaviour 12). The UISEC-9 scan allows `CGEventCreateCopy` in
  `native_click.rs`: it copies an event of IDECraft's own queue to set its button number and posts
  the copy to IDECraft only; it forbids `CGEventPost*`, `CGEventCreate{Mouse,Keyboard,ScrollWheel}
  Event`, event taps, XTest, `XSendEvent`, `SendInput` and `AXUIElementPostKeyboardEvent`.
- **20c — The native level on Linux (v5).** X11 and Wayland have no queue of the application's
  own that events can be posted to: the only native paths are the display server's (XTEST,
  `XSendEvent`, a virtual pointer of the compositor, uinput), and each of them puts input into the
  whole display, where any client and any window under the point could receive it. They stay
  forbidden (UISEC-9), also restricted to IDECraft's own windows and also in the self-test: the
  display decides who gets the input, not IDECraft. Instead the native level on Linux (and on
  every platform other than macOS) builds the toolkit's own pointer events — the pointer moves to
  the point, then each press and release with its click count, Control on a Control-click and a
  move without it afterwards — and hands them, after the current frame, to the window's input
  handler: the handler gpui's X11 and Wayland layers call for a real click. Nothing reaches the X
  server or the compositor. GPUI 0.2.2 has no public way to do that (16a), so the workspace uses
  gpui 0.2.2 vendored in `vendor/gpui/` with one addition, `Window::dispatch_in_process_pointer_input`
  (pointer events only; keys and file drops are refused), recorded in `vendor/gpui/IDECRAFT.md`.
  Only `crates/app/src/native_click.rs` calls it (scan test), and only the self-test reaches that
  file: external clients keep `unsupported` for the native level (20a) and for pointer actions at
  the window level (16a). What the platform layer adds to a real click and this path leaves out:
  its own click counting (the step gives the count) and the commit of an unfinished input-method
  composition; none of the self-test's checks depends on them. [Decision of the coordinator on open question 40, 2026-11-15]
- **20b — Keys on the layout.** A key with a modifier that the layout cannot type is pressed where
  the system puts it (the platform's key equivalents, as the self-test resolves `[`); Option
  combinations carry no character.
- **25a — Screenshots.** `ui_screenshot` answers `unsupported` (behaviour 25 allows it); no
  read-back of the frame exists in GPUI 0.2.2.
- **26a — Traces.** Traces have every field of behaviour 26 except element screenshots (25a) and
  stderr lines, which phase 1 does not capture. `ui_trace stop` writes into `ui-traces/<time>/`
  of the user's settings folder (IDECraft has no separate state folder yet) or into a folder of
  the workspace named by `path`.
- **28a — Self-test steps.** `Step::Click` and `Step::Hover` run on the driver when
  `IDECRAFT_SELFTEST_DRIVER=1` is set (default: the old path until the window run proves it):
  locator `id=<mark name>`, auto-waiting, delivery at the native level. `Step::Ui` and
  `Step::ExpectUi` come with the first group that moves (phase 2). `WaitFor` keeps its 16 ms sleep
  until phase 2 (behaviour 42); the driver crate and its window host have none (AK-3 scan).
- **30a — Settings place.** The switch "Allow agents and scripts to operate the window" is on the
  Settings page AI Assistant, under "Serve MCP to local agents over HTTP" (no page "Tools › UI
  Automation" exists yet). While the Settings dialog is open, external actions answer `reserved`:
  its pages for agent rights, MCP and UI automation could otherwise be operated with keys alone.
  The setting is never taken from an imported settings file and is not part of a settings bundle.
- **35a — Effects of keys.** `press` takes the effects of the keymap actions bound to each key,
  from a table of action ids (unknown ids are `undeclared`; any action id with `Paste` is
  `reserved`); an unbound key types into the focused node and takes its effects. Editing a
  document counts as `write`.
- **36a — Marking.** The marking stays until the emergency stop or 30 s after the client's last
  call, and disappears with the next frame after that. Screen-reader announcements come in
  phase 3.
- **37a — The user's own input.** Physical keys are seen through the window's keystroke
  interception, pointer input through listeners of the marking; both only while the marking is
  shown. Input the driver dispatches is marked while it runs and never counts.
- **31a — Session family `ui`.** Reserved, not served: a backend refuses every `ui.*` request with
  `denied` and logs it, no backend can bind the family, and the window answers `ui.*` requests of
  a backend with `denied` (UISEC-8). Served by the window's own process in phase 4.
- **44a — Ratchet.** Stand-ins are not marked yet, so the ratchet counts every construction of
  `Step::Act` in the self-test (599 at the first implementation, after merging #423) and every `hit_targets::mark`
  site (26, also after #423); both may only fall. Set-up that is not a user action uses the new `Step::Setup`,
  which is not counted.
- **Status of the AKs in phase 1:** AK-1, AK-3, AK-7, AK-10 (the engine and the Rust API without a
  screen), AK-11 and the MCP part of AK-8 are tests in `cargo test`; AK-2, AK-4, AK-5, AK-6 need
  the window and phase 2; AK-9 waits for 25a; AK-12 for the window bench; AK-13 for phase 3.

## Window check of phase 1 (v3)
The window check of 2026-11-07 drove a window over the MCP server on HTTP (isolated settings
folder, `ui` and `write` granted in the grant file) and ran the self-test groups `mouse` and
`tooltips` with `IDECRAFT_SELFTEST_DRIVER=1`. What holds and what follows from it: [GEN]

1. **Holds:** with the switch off every `ui_*` tool except status and stop answers `unavailable`
   with the place of the switch; `press` of a key with a declared effect is delivered (⌘⇧O opened
   Go to File) and answers with its effects; the 2-pixel frame and the chip "Automated by
   <client>" with Stop show while the client drives and change no layout; `ui_driver_stop` ends
   every driver session, switches `ui.driver.external` off in the settings file, removes the
   marking and writes the audit lines; "Stop UI Automation" is in the Tools menu; the groups
   `mouse` (29 checks) and `tooltips` (6 checks) pass on the driver (`tooltips` alone also needs the
   driver's auto-waiting: without it its first hover finds no painted target).
2. **Text does not reach the editor yet (phase 2):** no view is annotated yet (3a), so the tree
   holds only marks: no node is editable, `type` without a locator answers `invalid` and on a mark
   `undeclared`, and keys without a binding (letters) or bound to an action without a declared
   effect (Escape in a popup) answer `undeclared`. This is the safe reading of behaviours 18 and
   35a; it is lifted when the editor and the text fields are annotated (behaviour 6, phase 2).
3. **Error lines name their code once:** a message that already starts with its code
   (`unavailable: …`) is not prefixed a second time.
4. **Open (needs the user's own keyboard and pointer):** Esc as emergency stop, the 3-second
   pause after the user's input, the chip's Stop button and the Settings row are checked by hand;
   the check's computer use could not take the screen (the user was at the Mac).
5. **37b — Clients that went away (v4):** a driver session without a call for 60 s and without a
   call waiting ends and leaves `ui_driver_status`, with an entry `ui_session_ended` in
   `.idecraft/agent-log.jsonl`; the same client's next call lists it again. Phase 1 uses only this
   timeout: the MCP transport tells the window nothing when a client closes its connection.

## Known deviations from the reference
- **IMP-0382:** the reference platform's UI test tool (Remote Robot) serves an HTTP endpoint on a
  local port without authentication that executes scripts inside the IDE, addresses components by
  XPath over Swing class names, and waits by polling. IDECraft's driver has no listener of its own,
  is off by default, needs the right `ui` plus the effect rights, executes no scripts inside the
  process, addresses elements by role, accessible name and test id from the accessibility tree, and
  waits on events.
- The reference has no built-in, permissioned driver for agents; IDECraft offers one through MCP
  (ADR-0008).

## Open points
- Frame read-back for screenshots in the upstream GPUI pin (phase 3) and in GPUI 0.2.2 (phase 1);
  where missing, `ui_screenshot` stays `unsupported` until a small toolkit contribution lands.
- Whether GPUI 0.2.2's test platform lays out text well enough for AK-10 on macOS and Linux.
- Whether frames advance in a window that is fully occluded or on a locked screen once the driver
  requests them (behaviour 13), to be measured in phase 1.
