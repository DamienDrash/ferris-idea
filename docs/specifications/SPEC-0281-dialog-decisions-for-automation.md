# SPEC-0281: Dialog decisions for automation and assistive technology

- **Version:** 4 · **Date:** 2026-11-16 (v4: on Linux, where the toolkit draws the question,
  it answers the same keys as on macOS — Return the default, Escape the cancel answer, Space the
  focused answer, which starts on the safe answer; Tab and Shift+Tab move a visible focus ring —,
  keeps the keyboard focus and the pointer while it is shown, lets no held key and no typing
  answer anything but the safe answer, and is in the semantic tree with its answers reserved for
  the user, behaviour 9; the open accessibility point of v3, rule 12; v3: the keyboard focus of a native alert sits on its
  safe answer — the one the question names, else its cancel answer, else its default — so a stray
  Space neither grants nor destroys or closes anything, behaviour 8; the coordinator's decision of
  2026-11-16 under the general authority of 2026-11-13 named Preview in Safe Mode for the trust
  question — the pre-check of hand test 202
  found that with keyboard navigation on, Space in an agent's request for a right pressed "Always
  for This Workspace", the button the alert focused; rule 12; v2: questions that block quitting or
  closing a window are drawn, behaviours 2 and 6, after the coordinator's window run of 2026-11-10
  where the quit question never appeared, SPEC-0261 v4)
- **Sources:** OBS-0002, OBS-0003, OBS-0004 (three modal dialogs of the reference were missing from
  the macOS accessibility window list or ignored accessibility clicks → IMP-0005); STD (macOS:
  alerts drawn by the system expose their message and buttons to accessibility clients; Model
  Context Protocol, `tools/call`); GEN (a confirmation without input needs no custom drawing);
  ADR-0003, ADR-0008, ADR-0019; SPEC-0006, SPEC-0261, SPEC-0275, SPEC-0278
- **Feature matrix rows:** F-084, F-087

## Purpose
No decision IDECraft asks for in a modal dialog depends on a mouse and eyes. Until the toolkit has
an accessibility tree (ADR-0019), each decision is reachable through MCP and the command line, and
simple confirmations are native alerts that screen readers can read and press.

## Behaviour

### Inventory
1. **One machine-readable list.** Every modal dialog IDECraft shows is listed with its
   specification, how it is shown (native alert or drawn by IDECraft), its answers, and for each
   answer the MCP tool and arguments that make the same decision. The MCP tool
   `dialog_decisions` returns the list; with `id` it returns one dialog. [ADR-0008]
2. **Dialogs in the list (version 2).** Native alerts: trust project (SPEC-0278), an agent's
   request for a right (SPEC-0053 v4), delete from the project tree (SPEC-0275, behaviour 6 here).
   Drawn by IDECraft: quit with unsaved files or running work (SPEC-0261 v4), close a remote
   project with unsaved files (SPEC-0317 behaviour 11), command suggested by a plugin (SPEC-0276), rename table or column (SPEC-0128), copy
   table (SPEC-0191), breakpoints (SPEC-0127), attach to process (SPEC-0147), database connection
   (SPEC-0108), push (SPEC-0109/0144), update project (SPEC-0144), interactive rebase (SPEC-0145),
   rename preview (SPEC-0148), settings (SPEC-0105/0129), unsafe SQL change (SPEC-0192),
   destructive schema changes (SPEC-0110), new file name, rename file, paste name (SPEC-0275),
   merge conflicts (SPEC-0101), stashes (SPEC-0106), delete on a remote host (SPEC-0253),
   change signature (SPEC-0113), plugin installation (SPEC-0126/0211). Popups that only navigate
   (Search Everywhere, Go to, recent files, branches, themes) are not modal decisions. [GEN]
3. **Every answer has a tool**, except answers a security specification reserves for the user:
   installing a plugin and granting its rights (SPEC-0126, SPEC-0211), "Always Allow in This
   Workspace" for a plugin's command (SPEC-0276) and answering an agent's request for a right
   (SPEC-0053 v4). The list names the reason and what an agent can read instead (`plugins`,
   `inspect_plugin_file`, `run_plugin_configuration` with `dry_run`, `agent_permissions`).
   Quitting is `quit_ide` and `idecraft quit` (SPEC-0261 v2). [ADR-0008, SPEC-0276]
4. **Kept true by tests.** A test fails when the list names a tool the server does not have, names
   a test that does not exist, or when the workbench has a dialog type (`…Dialog`) that the list
   does not name. [GEN]

### Command line
5. **`idecraft call <folder> <tool> [<json arguments>]`** calls one MCP tool on the folder as
   `idecraft mcp <folder>` would serve it — same agent rights (SPEC-0053), same safe mode for an
   untrusted folder (SPEC-0278) — prints the tool's result as JSON on standard output and exits
   with 0; when the tool reports an error it prints the error on standard error and exits with 1;
   wrong usage, a folder that cannot be opened or arguments that are not a JSON object exit
   with 2. So every decision in the list is also reachable from a shell. Trust also keeps its
   own commands (`idecraft trust …`, SPEC-0278). [ADR-0008]

### Native alerts
6. **Simple confirmations are native alerts.** A question whose answers are only buttons (no
   field, list or preview) is asked with the platform's alert: on macOS a system alert attached
   to the window, which accessibility clients read with its message and buttons; on Linux the
   toolkit draws it. Version 1 moves the project tree's Delete confirmation (SPEC-0275 behaviour
   6) to a native alert: message "Delete file "…"?" (or the folder with its counts), detail
   "Deleted files stay in Local History.", buttons **Delete** (default, Return) and **Cancel**
   (Escape). [STD, IMP-0005]
   **Exception (v2):** a question that keeps IDECraft from quitting or a window from closing is
   drawn in the window, with the same message, detail, buttons and keys: quitting (SPEC-0261 v4
   behaviour 5) and closing a remote project with unsaved files (SPEC-0317 behaviour 11: "n remote
   files are unsaved", the files named, **Save All** (default), **Don't Save**, **Cancel**). In the
   coordinator's window run of 2026-11-10 (macOS 27, Stage Manager on) the quit question as a
   system sheet never appeared and nothing quit; a sheet that does not appear must never be the
   only way out. Their decisions stay available through MCP (`quit_ide`, `remote_project_close`).
   [GEN]
7. **Window self-test.** The self-test cannot answer a native alert, so it shows the same question
   drawn in the window, answered with the same keys and the same code path. [GEN]
8. **Keyboard focus (v3).** In every native alert IDECraft shows, the button that holds the
   keyboard focus when the alert appears is the question's safe answer, so that with keyboard
   navigation on — where Space presses the focused button — a stray Space neither grants anything
   (trust, rights, pairing) nor removes or closes anything. A question may name its safe answer:
   the one that grants nothing and removes or closes nothing. The focus goes to that named answer;
   without one, to the question's cancel answer (the one Escape gives); without that, to its
   default (the first answer, the one Return gives). Return keeps giving the default and Escape
   the cancel answer; any other answer needs the mouse, VoiceOver or Tab first. The focus per
   question: an agent's request for a right names Deny (also its default); the trust question
   names Preview in Safe Mode (also its default; it grants no trust and changes nothing — Don't
   Open, its cancel answer when the window opens, would close the window and may quit IDECraft),
   both when the window opens and when asked again from the safe-mode banner; Delete in the
   project tree — Cancel (Return stays Delete); pairing with a remote host — Cancel (Return stays
   Digits Match); closing tabs with unsaved changes — Cancel (Return stays Save All); a plugin's
   command on a file it could not read — Cancel, also its default; About — OK. The buttons stand
   in the order the question lists them; the platform may draw the button Escape gives last. On
   Linux the toolkit draws the question and follows the same rule (behaviour 9). [STD (macOS:
   Return presses an alert's first button, Escape the button bound to it, Space the focused button
   when keyboard navigation is on), GEN, rule 12, coordinator's decision 2026-11-16]
9. **The question the toolkit draws (v4).** Where the platform has no alert of its own (Linux,
   X11 and Wayland), the toolkit draws the question of behaviour 6 over the window: the message,
   the detail below it and one button per answer, top to bottom in the order the question lists
   them (the order of behaviour 8 and of each question's specification; nothing is moved). It
   answers the keyboard as the macOS alert does with keyboard navigation on:
   1. **Return** (also Enter on the keypad) gives the default, the first answer. **Escape** gives
      the cancel answer; a question without one (an agent's request for a right, the trust
      question asked again from the safe-mode banner) stays open on Escape.
   2. **The focus** starts on the safe answer of behaviour 8 — the one the question names, else
      the cancel answer, else the default — and a focus ring, a 2-pixel blue frame around the
      button, shows it. **Space** gives the focused answer.
   3. **Tab** moves the focus to the next answer, **Shift+Tab** to the previous one; both wrap
      around. The ring follows.
   4. **No stray key grants (rule 12).** Only these keys answer or move the focus, and only
      without Control, Alt, the platform key or Fn (Shift only with Tab). A key the platform
      reports as held (auto-repeat: Wayland does; on X11 gpui 0.2.2 reports each repeat as a new
      press, see Out of scope) does nothing, so a key that was down before the question appeared
      does not answer it. A key that types a character moves the focus back to the safe answer and does
      nothing else: whoever types on into a question that appeared while they typed — Tab for an
      indent, a word, then Space — gives at most the safe answer. No key reaches the window
      behind the question.
   5. **Modal.** While the question is shown it keeps the keyboard focus: a view that asks for
      the focus meanwhile gets it once the question is answered (instead of the view that had it
      before). The pointer reaches only the question; the window behind it is covered (clicks,
      hover, the wheel).
   6. **Semantic tree.** While a consumer of the tree is active (SPEC-0319: the self-test, the UI
      driver), the question is in the window's tree in AccessKit's schema: a cover node over the
      window, a modal node of role `alertdialog` named by the message with the detail as its
      description (test id `native-prompt`), and per answer a `button` named by its label (test id
      `native-prompt-answer-<n>`, n from 0 in the order of the answers), the focused one marked
      focused, the default with the keyboard shortcut `Enter`, the cancel answer with `Escape`.
      The question and its answers are **reserved for the user** (`reserved: decision`, SPEC-0319
      behaviour 35): an external client of the UI driver can read them but neither press, click
      nor type into them, and while such a question is open in a window the driver delivers no
      input of an external client into that window, checked again before each key. The decisions
      stay reachable through MCP and the command line (behaviour 3). A screen reader reaches the
      tree once the toolkit has a platform adapter (ADR-0019, ADR-0027 phase 3).
   [STD (WAI-ARIA Authoring Practices, alert dialog pattern: the focus moves into the dialog and
   stays there, Tab and Shift+Tab cycle through its controls, Escape closes it; AccessKit roles
   and the `modal` and `keyboard_shortcut` properties), GEN, rule 12]

## Acceptance criteria
- **AK-1** The list: every named tool exists in the server, every named test exists, every
  `…Dialog` type of the workbench is listed, ids are unique, every answer has a tool or a reason
  (`dialogs::tests`).
- **AK-2** MCP: `dialog_decisions` lists every dialog, `id` selects one, an unknown id is an
  error, and every tool it names is in `tools/list` (`crates/mcp/tests/it/protocol.rs::dialog_decisions_name_existing_tools`).
- **AK-3** Command line: `idecraft call` prints a tool's JSON, reports a tool error with exit
  code 1 and wrong usage with 2 (`crates/app/tests/it/call_cli.rs`).
- **AK-4** The Delete alert's message, detail and buttons (`workbench_menus::tests::delete_alert_question_and_buttons`).
- **AK-6** The drawn question: keys and buttons, quit and remote close listed as drawn
  (`dialogs::tests`, `crates/mcp/tests/it/protocol.rs::dialog_decisions_name_existing_tools`).
- **AK-5** Hand test (round 5): Delete in the project tree asks in a system alert; VoiceOver
  (⌘F5) reads the question and both buttons; Return deletes, Escape keeps the file.
- **AK-7 (v3)** The focused answer of a native alert is the answer it names as safe, else its
  cancel answer wherever it stands, else its first answer; the agent's request for a right
  focuses Deny, the trust question Preview in Safe Mode also when it has Don't Open, the pairing
  question Cancel, and in a window of the toolkit's test platform Delete in the project tree
  focuses Cancel (pressing it keeps the file) and closing tabs with unsaved changes focuses
  Cancel (`crates/app/src/mcp_http.rs` tests, `trust_model` tests, `workbench_remote_project`
  tests, `crates/workbench/tests/it/native_prompt_focus.rs`).
- **AK-8 (v3)** Window check on macOS: with keyboard navigation on, the focus ring of an agent's
  request for a right is on Deny and Space answers it — the call is refused with `denied:` and
  the grant file stays unwritten; in the Delete question of the project tree the ring is on
  Cancel and Space deletes nothing, while Return still deletes; in the trust question when the
  window opens the ring is on Preview in Safe Mode and Space opens the project in safe mode.
  Hand test with keyboard navigation switched on in System Settings.

- **AK-9 (v4)** The question the toolkit draws, on the toolkit's test platform: the focus starts
  on the safe answer, else the cancel answer, else the default; Return gives the default, Escape
  the cancel answer and nothing without one, Space the focused answer; Tab and Shift+Tab move the
  focus and wrap; a held key, a key with Control or the platform key and a typed character give
  nothing, and a typed character moves the focus back to the safe answer (Tab, a letter, Space
  in an agent's request for a right gives Deny); keys never reach the view behind; a view asking
  for the focus while the question is shown gets it after the answer; the answers stand top to
  bottom in the order given; for the agent's request for a right, the trust question (with and
  without Don't Open) and Delete in the project tree each key gives the answer behaviour 8 names;
  the semantic tree holds the reserved, modal `alertdialog` with its buttons, the focused one
  marked (`crates/workbench/tests/it/fallback_prompt_keys.rs`); an external driver call while
  such a question is open answers `reserved` (`crates/uidriver` tests).
- **AK-10 (v4)** Window self-test on Linux (group `prompt-keys`, X11 on Xvfb in the VM of
  `docs/quality/QG-1-LINUX-WINDOW.md`): the platform's own path draws the question; keys sent
  inside the app — never through the X server — give in an agent's request for a right Deny for
  Return and Space, nothing for Escape, Allow Once only after Tab; in the trust question Preview
  in Safe Mode for Return and Space, Don't Open for Escape on opening; in Delete in the project
  tree Cancel for Space and Escape (the file stays) and Delete for Return; Tab and Shift+Tab move
  the focus with the ring as behaviour 9 says.

## Out of scope
An accessibility tree for drawn dialogs, tool windows and the editor (ADR-0019, waits for a
toolkit decision); answering an open dialog of a running window through MCP (the decision is made
directly instead).

**Open (v3), accessibility follow-up — closed in v4:** on Linux the toolkit's fallback question
could not be answered by keyboard; behaviour 9 specifies its keys, focus and tree. Still open: a
screen reader on Linux (Orca over AT-SPI) reads the question only once the toolkit has a platform
adapter for the tree (ADR-0019, ADR-0027 phase 4). On X11, gpui 0.2.2 reports a held key's
repeats as new presses, so a key held down when a question appears there acts as pressed again:
Return gives the default and Space the safe answer, which for an agent's request for a right and
the trust question grant nothing; telling repeats apart on X11 (detectable auto-repeat) is a
change of the toolkit's X11 backend.

## Known deviations from the reference
The reference's modal dialogs were not reachable for automation (OBS-0002 … OBS-0004); IDECraft
offers every decision as an MCP tool and on the command line, and asks simple confirmations in
native alerts (IMP-0005).
