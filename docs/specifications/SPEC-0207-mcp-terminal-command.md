# SPEC-0207: Running a command in a terminal from MCP

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (IDE MCP servers offer a tool that runs a shell command in the IDE's terminal and
  returns its output), the project owner's decision of 2026-10-31 (question 3: MCP clients may start commands,
  with the workspace's own right and a preview by default), SPEC-0053 (rights, agent log),
  SPEC-0038 (session and screen), SPEC-0206 (shell integration)
- **Feature matrix rows:** F-076

## Purpose
`run_command` (SPEC-0053) starts one program without a shell. Agents also need a shell command line
(pipes, `&&`, globbing, aliases of the user's shell) with output as a terminal shows it.

## Behaviour
1. **Tool:** `terminal_command` takes `command` (one shell command line), optional `cwd` in the
   workspace, `timeout` in seconds (default 120, at most 600), `rows`/`cols` (default 40 × 120) and
   `dry_run`. [ADR-0008]
2. **Preview:** `dry_run` defaults to true and returns the shell, the command line, folder, timeout and
   size without starting anything. [Decision 2026-10-31]
3. **Run:** `dry_run=false` needs the `run` right. The command runs as `<shell> -c <command>` in a
   pseudo terminal (`$SHELL`, else `/bin/sh`; `TERM=xterm-256color`), in its own process group. The
   result is the exit code (null after the timeout, which ends the group), `timed_out`, the duration
   and the output as the terminal shows it: scrollback and screen as text without trailing empty
   lines, capped at the last 256 KiB, with `truncated`. [GEN, SPEC-0053]
4. **Log:** every run is logged in `.idecraft/agent-log.jsonl` (command, folder, exit code, timed out),
   like `run_command`. [SPEC-0053]

## Acceptance criteria
- **AK-1** Output as shown by a terminal (colours stripped, `\r` overwrites applied) and the exit code of
  a real command; a timeout ends the command (`pty` integration tests).
- **AK-2** Preview by default, refused without `run`, run and logged with it (protocol test).

## Out of scope
Running the command in a tab of an open IDE window (the MCP server runs without the window), input
to the running command.

## Known deviations from the reference
The reference's tool runs in its terminal window; ours runs headless with the same output a terminal
would show.
