# SPEC-0313: Terminal, run output and problems in the AI chat, with consent

- **Version:** 2 · **Date:** 2026-11-15 (v1 2026-11-05; v2: a long chip wraps and keeps its ×;
  assignments inside a quoted word are masked — found by the re-check of hand tests 213 and 214
  without a screen)
- **Sources:** ADR-0023 rule 15 (the assistant sees what the person sees, with consent; the
  read-only part and "Explain with AI" belong to F-082 and may be built before QG-1, the project owner's
  approval of 2026-11-05) and rule 7 (untrusted content, no secrets, read-only first, local audit),
  GEN (assistants in IDEs and terminals offer "explain this error" on output, attach output to a
  question, and suggest shell commands the user runs; secret scanners recognise tokens by their
  published prefixes, `NAME=value` lines with secret-looking names, PEM private key blocks and
  credentials in URLs), STD (OSC 133 prompt marks as in SPEC-0206; RFC 3986 user information in
  URLs; PEM armour of RFC 7468; bracketed paste), SPEC-0138, SPEC-0143, SPEC-0157, SPEC-0160,
  SPEC-0162, SPEC-0206, SPEC-0087, SPEC-0088, SPEC-0092, SPEC-0032, SPEC-0053, ADR-0007, ADR-0008
- **Feature matrix rows:** F-082

## Purpose
The person can hand the assistant what they are looking at — a terminal's output, the last command
and its exit code, a run's console, a failed test, the problems of a file, the Git diff — and ask
about it, without copying and pasting, without anything being read that they did not choose, and
without secrets leaving the machine. The assistant can suggest a command, which the person runs.

## Behaviour
1. **Attachments are chips.** Everything handed to the chat becomes an attachment shown as a chip
   above the question field, like attached code (SPEC-0138 behaviour 3). A chip names its source
   and size: for example `Terminal "Local" · last command · cargo test · exit 101 · 38 lines`,
   `Run "tests" · 412 lines`, `Test calc › divides`, `Problems src/a.rs · 3`, `Git diff · 2 files`,
   plus `n masked` when secrets were masked and `truncated` when it was cut. Several chips can wait
   at once (at most 8); each has a × that removes it, and Escape in the empty question field removes
   the last one. A chip is a snapshot taken at the moment of the action; it does not change while it
   waits. The question sends every chip, in order, as a fenced block under its label, then the
   question text. Code attachments of SPEC-0138 become chips the same way. **v2:** a chip longer
   than the chat is wide wraps inside it; its × always stays visible and clickable.
   [GEN, ADR-0023]
2. **From a terminal tab** (its context menu, `Attach to Chat ›`, and Find Action):
   - **Selection:** the selected text.
   - **Last Command:** when the shell marks prompts (OSC 133, SPEC-0206 shell integration), the
     last command that ran: its command line, its output (from the output-start mark to the next
     prompt) and its exit code; a command still running gives its output so far and `running`.
     Without marks, or before the first command, this entry is labelled **Last n Lines** instead
     and takes those lines. [STD, GEN]
   - **Last n Lines:** the last *n* lines of the tab's history (scrollback and screen, without
     trailing empty lines). *n* is the setting "Terminal lines to attach" in Settings › Tools ›
     AI Assistant (`ai.terminal_lines`, default 200, 10 to 5 000). [GEN]
3. **From other tool windows:**
   - **Run:** `Attach Output to Chat` in the Run console's context menu attaches the selected
     tab's console (command line, output, the closing exit-code line). [GEN]
   - **Test failure:** on a failed or erroneous test in the test results tree, `Attach to Chat`
     attaches the test's full name, its message and its details or stack trace. [GEN]
   - **Problems:** `Attach to Chat` in the Problems tool window attaches the selected problem, or
     all problems listed for the file when none is selected: file, line, column, severity and
     message per problem. [GEN]
   - **Git diff:** `Attach Git Diff to Chat` (Find Action, VCS menu) attaches the working tree's
     diff against `HEAD` (staged and unstaged changes of tracked files) and the names of untracked
     files. [GEN]
4. **Explain with AI.** The context menus of the terminal (on a selection, else on the last
   command) and of the Run console (its output), and a failed test's menu, offer
   `Explain with AI`. It opens the chat, adds the chip and puts a question into the field: "Explain
   this output. If it shows an error, say what caused it and how to fix it." for output and "Why
   does this test fail, and how can it be fixed?" for a test. The question is not sent until the
   person presses Enter, so they can change it or remove the chip. [GEN]
5. **Consent.** A terminal tab is never read except by the person's own action: one of the entries
   of behaviours 2 and 4, or **Share Tab with This Chat** in the tab's context menu. While a tab is
   shared, every question sent from the chat takes the tab's last command (or last *n* lines) at the
   moment of sending as a chip, once per change: an unchanged snapshot is not sent twice. A shared
   tab shows an `AI` marker in its tab title (and the tab's tooltip names the chat), and the chat
   shows `Sharing: "Local"` above the question field. Sharing ends with **Stop Sharing with Chat**,
   with New Chat, when the tab is closed, and when the assistant is switched off. Sharing is never
   saved; a restarted IDE shares nothing. [ADR-0023]
6. **Secrets are masked** before a chip is made, so the chip, the request and every MCP answer of
   behaviour 9 carry only the masked text. Each found secret is replaced by `‹masked›`, keeping
   what tells the reader what it was:
   - PEM private key blocks (`-----BEGIN … PRIVATE KEY-----` to the matching end line), whole;
   - tokens with published prefixes: GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`,
     `github_pat_`), GitLab (`glpat-`), Slack (`xoxb-`, `xoxp-`, `xoxa-`, `xoxr-`, `xoxs-`),
     OpenAI and Anthropic keys (`sk-`, `sk-proj-`, `sk-ant-` followed by at least 20 key
     characters), Stripe (`sk_live_`, `rk_live_`, `sk_test_`, `rk_test_`), Google API keys
     (`AIza` and 35 characters), AWS access key ids (`AKIA`/`ASIA` and 16 capitals or digits), npm
     (`npm_`), PyPI (`pypi-`), Hugging Face (`hf_`), JSON Web Tokens (three base64url parts, the
     first two starting with `eyJ`);
   - `Authorization:` and `Proxy-Authorization:` header values after their scheme
     (`Bearer ‹masked›`), and `-u user:password` arguments (`-u user:‹masked›`);
   - assignments whose name looks secret — it contains `PASSWORD`, `PASSWD`, `PWD` as a whole
     part, `SECRET`, `TOKEN`, `API_KEY`/`APIKEY`, `ACCESS_KEY`, `PRIVATE_KEY`, `CREDENTIAL`,
     `AUTH` or `SESSION_KEY`, in any case and with `_`, `-` or `.` between parts: `NAME=value`,
     `export NAME=value`, `NAME: value`, `"name": "value"`, `name = "value"` and command-line
     options `--name=value` / `--name value`; the name stays, the value is masked (empty values
     and values that are already `‹masked›`, `***`, `$NAME`, `${NAME}` or `<…>` placeholders stay,
     and so do the shell's `PWD` and `OLDPWD`, which hold folders); **v2:** also inside a quoted
     word of a command line, `printf 'API_TOKEN=x'` or `echo "DB_PASSWORD=x"`;
   - credentials in URLs: `scheme://user:password@host` becomes `scheme://user:‹masked›@host`.

   Masking works line by line on plain text, is idempotent (masking masked text changes nothing),
   and counts what it masked. It applies to every attachment source, code included. It is a safety
   net, not a promise: the chip shows the text that will be sent. [GEN, ADR-0023 rule 7]
7. **Size budget.** One attachment is at most `ai.attachment_chars` characters (default 24 000,
   about 6 000 tokens at four characters per token; 1 000 to 200 000). A longer text keeps its head
   (the first third of the budget) and its tail (the rest), cut at line ends, with one line between
   them: `‹… n lines (m characters) left out …›`. All chips of one question together are at most
   four times the budget; a chip that would exceed it is refused with a note in the chat. Truncation
   happens after masking. [GEN]
8. **Suggested commands.** In the assistant's answers, a fenced block whose language is `sh`,
   `bash`, `zsh`, `shell`, `console` or `terminal` shows an **Insert into Terminal** action. It
   types the command into a terminal tab — the shared tab, else the tab of the newest terminal chip
   of the conversation, else the active terminal tab, else a new tab — and focuses it, **without
   pressing Enter**: the person reads it and runs it. Before insertion a leading `$ ` or `% ` prompt
   is removed from each line, trailing line ends are dropped, and control characters other than
   tab and line end are removed, so the text cannot press keys. A command of several lines is
   inserted only when the shell has turned on bracketed paste (then it arrives as one paste, which
   shells do not run on their own); otherwise nothing is inserted and the chat says why. Running a
   command for the person, approvals per command and command patterns are out of scope here (ADR-0023
   rule 15, M11). [GEN, STD]
9. **MCP** (ADR-0008, ADR-0023 rule 7). All three tools are read-only and mark terminal and run
   text as untrusted data in their descriptions:
   - `terminal_read`: without `tab`, the window's terminal tabs (index, name, whether shared with
     the chat, whether the shell marks prompts) and no content; with `tab` (name or 0-based index)
     and `mode` `last_command` (default) or `lines` (`lines`, default the setting), the masked and
     budgeted text, the command line and exit code (last command), `masked` and `truncated`. It
     needs the agent right `terminal-read` (SPEC-0053; asked in the window over HTTP, refused with
     `not granted:` otherwise), and every read is written to the local agent log. Without a window
     (stdio server) there are no tabs.
   - `run_output`: the Run tool window's tabs (name, running, exit code) and, for `tab`, its masked
     and budgeted console text and its failed tests (name, message, details). Read-only, needs no
     right; nothing without a window.
   - `ai_assistant` (SPEC-0138) also reports `terminal_lines`, `attachment_chars`, the chips
     waiting in the chat (`attachments`: source, label, lines, characters, masked, truncated — never
     their text) and the names of shared tabs. Agents still cannot add chips, share tabs or send.
10. **Limits.** Attachments become part of the user message, so they work with every provider of
    SPEC-0138, SPEC-0143 and SPEC-0160 alike. While the assistant is off, the attach, explain and
    share entries are shown disabled and nothing is read; nothing is sent. No telemetry
    (ADR-0007). [ADR-0007]

## Acceptance criteria
- **AK-1** Masking: every pattern of behaviour 6 masked with what it keeps; ordinary text, hashes,
  paths, empty and placeholder values left alone; idempotence; the count (`idecraft_ai::context`
  unit tests).
- **AK-2** Budget: short text unchanged; long text keeps head and tail at line ends with the
  left-out note; one huge line; the total cap of a question (`context` unit tests).
- **AK-3** Last command: OSC 133 marks give command, output and exit code; a running command;
  a command whose prompt left the history and output without marks give the last *n* lines
  (`idecraft_terminal` screen tests).
- **AK-4** Chips: labels with source and size, several chips sent in order before the question,
  removal, the shared-tab snapshot taken once per change; command blocks found by language and
  cleaned for insertion (prompt, control characters, several lines with and without bracketed
  paste) (`context` unit tests).
- **AK-5** MCP: `terminal_read` refused without `terminal-read`; with it, a registered tab's last
  command and lines come back masked and the read is logged; `run_output` without a window is
  empty; `ai_assistant` reports the new settings and no attachments by default
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-6** Window self-test: the terminal's context menu offers the entries; attaching the last
  command of `echo` with a fake token shows a chip with `exit 0` and `1 masked`; Explain with AI
  prefills the question without sending; Enter sends the chip to the fake endpoint with the token
  masked; sharing marks the tab and the next question carries the new output; stopping removes the
  marker; Insert into Terminal types an answer's command without running it.
- **AK-7** Hand test (round 5): with a real provider — `cargo test` failing, Explain with AI on the
  terminal, a failed test and the Run console; a `.env` printed with `cat`; Insert into Terminal.
- **AK-8** (v2) Window test without a screen with a real zsh and a fake endpoint: no terminal text
  before an action; Attach to Chat › Last Command through the context menu with the mouse, the
  chip with source, exit code, size and the masked token, × removes it; Explain with AI prefills
  and sends on Enter only; Explain Failure with AI on a failed TAP test and Attach Output to Chat;
  Problems and Git diff chips; sharing marks the tab, sends `ls -la` once; Stop Sharing; Insert into
  Terminal types without running; off, every AI entry disabled
  (`crates/workbench/tests/it/ai_assistant_handtests.rs`:
  `hand_tests_213_and_214_terminal_run_problems_and_diff_in_the_chat`); masking inside quoted words
  (`context::tests::secret_named_assignments_keep_the_name`).

## Out of scope
Running commands for the person and approvals for it (ADR-0023 rule 15, M11); hosted agents and
their terminals (ACP, M11); debugger state as an attachment; a live stream of a tab into the chat;
selecting lines in the Run console; secret detection by entropy alone; masking the editor's own
display.

## Known deviations from the reference
The reference's AI features are not examined (RESEARCH_BOUNDARIES). Secrets are masked before any
attachment leaves the IDE, and a terminal is only read by the person's action, per attachment or
while shared with a visible marker (IMP-0299). Suggested commands are typed, never run.
