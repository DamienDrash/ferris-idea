# SPEC-0357: Agents in the window

- **Version:** 1 · **Date:** 2026-11-23
- **Sources:** `OBS` OBS-0121 (the reference's agent sessions: agent choice, approval cards with
  the command or the diff of an edit, a session list with "needs input", the files a turn changed
  with undo; Ferris IDEA's host reachable only over MCP, approvals impossible); `GEN` (Agent Client
  Protocol version 1; programs that speak it on stdio: Gemini CLI with `--acp`, Antigravity with
  `acp`, the ACP adapters for Claude Code and Codex); SPEC-0348 v2 (the agent host, its states,
  changes and undo); SPEC-0349 behaviour 7 (approving is the person's decision); SPEC-0138 (the AI
  Chat tool window); SPEC-0319 v7 (UI driver, declared effects); ADR-0008 (every feature has an
  MCP tool); ADR-0017 (credentials stay with the provider's program); ADR-0023 rules 7 and 9;
  IMP-0546 to IMP-0549.
- **Feature matrix rows:** F-082 (KI-Assistenz), gaps A7, A8, A9, A10, A11 of OBS-0121.

## Purpose
A person wants to work with the coding agent of their choice inside Ferris IDEA: pick it, start
it, give it a task, see what it does, decide every edit and every command it asks for while seeing
the command or the diff, see which files the turn changed and undo the turn. Until now the agent
host of SPEC-0348 was reachable only over MCP, and an approval needed a window question that no
window could show, so a hosted agent could never change anything (OBS-0121 F4).

## Behaviour

### Where
1. The AI Chat tool window has two tabs in its header, **Chat** (SPEC-0138, as before) and
   **Agents**. The Agents tab belongs to the window's workspace and uses the agent host the
   window's in-process MCP server uses (one host per workspace and process), so an agent started
   over MCP is seen in the window and the other way round. [SPEC-0348 behaviour 31]

### Choosing and starting an agent (A11)
2. The tab lists the agents of `agent-host.toml` in the user's settings folder (SPEC-0348
   behaviour 11) by name, with a **Start** button each. A configuration problem is shown in
   words, and nothing can be started then. [SPEC-0348]
3. Below, **Found on this computer** lists ACP programs that are installed but not configured:
   Gemini CLI (`gemini`, argument `--acp`), Antigravity (`agy`, argument `acp`), the Claude Code
   ACP adapter (`claude-code-acp`) and the Codex ACP adapter (`codex-acp`), looked up in the
   folders of `PATH` and in `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`, only as
   regular executable files. **Add** writes one `[[agent]]` table with the program's absolute path
   and its argument into `agent-host.toml` (creating the file with mode 0600), keeping everything
   else in the file; an id already taken gets a number. Nothing is added without that click, and
   no tool call can add a program (SPEC-0348 behaviour 11 stays). [GEN, OBS-0121 R3]
4. Signing in stays with the agent's own program; a failed start shows the host's reason, which
   says so (SPEC-0348 behaviour 3). [ADR-0017]

### Following an agent (A7)
5. Each hosted agent has a row with its name, its id and its state in words — Working, Needs
   approval, Needs input, Idle, Done, Failed, Unknown — and **Stop** while it runs. Clicking a row
   selects it; a newly started agent is selected. [ADR-0023 rule 9, OBS-0121 R9]
6. The selected agent's output is shown oldest first: prompts, messages, thoughts (dimmed), tool
   calls, plans, permission requests and answers, refusals, stderr (dimmed), host notes and
   changes, each with its kind; agent text is shown as text, never as markup or links, with
   secrets masked (SPEC-0313 behaviour 6). The view follows the newest entry. [SPEC-0348
   behaviour 17]
7. A prompt field under the output sends with Enter (Shift+Enter: new line) or **Send**, only while
   the agent waits for input or is idle; otherwise the field says why. [SPEC-0348 behaviour 6]

### Approving (A8, A9)
8. Each waiting permission request of the selected agent is a card above the prompt field: its
   title and kind, the mark "You refused a similar request in this turn" when it has one
   (SPEC-0348 behaviour 25), and for each proposed edit its file and its lines as a diff (removed
   lines with `-`, added with `+`, unchanged context dimmed, at most 400 lines per card, then a
   note how many more). The buttons are the request's options of kind `allow_once` and
   `reject_*` with the agent's labels; an option of kind `allow_always` is never shown, and there
   is no way to approve future requests. [IMP-0546, IMP-0547, OBS-0121 R5d, R7]
9. Approving is the person's decision: the Allow button carries the effect reserved for the user,
   so the UI driver and every agent operating the window cannot press it (SPEC-0319); Reject and
   Stop can be pressed by them. Nothing is answered on a timer or by default. [SPEC-0349
   behaviour 7, ADR-0023 rule 7]
10. The tool window's tab title shows the number of agents that need the person (approval or
    input) in parentheses, e.g. "Agents (1)". [OBS-0121 R7]

### What a turn changed (A10)
11. After each turn the output holds the host's summary; under the output, for the newest kept
    turn of the selected agent with changes, a block lists its files with kind and `+added −removed`
    (or "binary"), and an **Undo turn N** button. Undo goes through the host (SPEC-0348 behaviour
    28): all or nothing; a refusal names the files that block it. Clicking a file opens it in the
    editor. [IMP-0548, IMP-0549, OBS-0121 R5a–R5e]

### MCP
12. `agent_host_detect` (reads, needs no right) answers the ACP programs found (behaviour 3) with
    name, program path, argument and whether `agent-host.toml` has them already. It never adds
    one. Starting, prompting, reading, answering, changes and stopping stay the tools of SPEC-0348.
    [ADR-0008]

## Acceptance criteria
- **AK-1** Given a settings folder with a fake ACP agent in `agent-host.toml`, when the person
  opens the Agents tab, then the agent is listed with Start; Start shows a row "Needs input";
  typing a prompt and Enter shows the prompt and the agent's answer and the row "Idle"; Stop shows
  "Done" (behaviours 1, 2, 5–7).
- **AK-2** Given a prompt that makes the agent ask for an edit with a diff, then a card shows the
  title, the file and the `-`/`+` lines, Allow once and Reject but no Always allow; the tab title
  counts one agent needing the person; Reject leaves the file unchanged and the agent's next,
  similar request carries the mark; Allow once (pressed by a person in the window) lets the turn
  end (behaviours 8–10).
- **AK-3** Given a turn that wrote a file and created another through a command, then the changes
  block lists both with Undo turn; Undo removes both; the UI driver can press Reject and Stop but
  its press of Allow is refused as reserved for the user (behaviours 9, 11).
- **AK-4** Given programs named `gemini` and `agy` in a folder of `PATH`, then they are offered
  under Found on this computer and by `agent_host_detect`; Add writes an `[[agent]]` with the
  absolute path and argument into `agent-host.toml` (mode 0600) and the agent moves to the
  configured list; a non-executable file is not offered (behaviours 3, 12).

## Out of scope
- Context chips for agents (A13), model, effort and mode selection (A14), signing in through the
  agent (`authenticate`, A15), a session list with titles, sounds, sessions kept across restarts
  (A16).
- Passing Ferris IDEA's MCP tools to hosted agents (A12): needs rights per agent (M19.1, M19.6).
- A side-by-side diff view of a turn (the block names the files; the unified diff is in MCP
  `agent_host_changes`).
- Installing agents from a catalogue: Ferris IDEA offers only programs already installed.

## Known deviations from the reference
- **D1** The reference's default agent mode edits files and runs commands without asking (OBS-0121
  R5a, R5d). Here every request waits for the person, and there is no mode without approvals
  (behaviours 8, 9; IMP-0546).
- **D2** The reference offers "Always allow" with broad patterns (`cd *`). Here no standing
  approval exists (behaviour 8; SPEC-0348 D1; IMP-0547).
- **D3** The reference's undo misses files changed by commands and once found no turn (OBS-0121
  R5c). Here the changes come from the workspace itself and Undo is all or nothing (behaviour 11;
  SPEC-0348 D4; IMP-0548).
- **D4** The reference keeps agents in editor tabs and a session list with history; here the
  Agents tab of the AI Chat tool window shows the agents of this run.
- **D5** The reference installs agents from a catalogue; here only installed programs are offered,
  and only the person adds them.
