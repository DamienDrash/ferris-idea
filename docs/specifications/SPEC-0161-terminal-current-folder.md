# SPEC-0161: File links relative to the shell's current folder

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (terminals resolve relative paths in output against the shell's current folder),
  STD (OSC 7 `file://host/path`, percent-encoded, as shells and terminals use it), SPEC-0153
- **Feature matrix rows:** F-076

## Purpose
After `cd sub` a compiler's `src/main.rs:3:5` must open `sub/src/main.rs`, not a file of the same
name under the folder the terminal started in.

## Behaviour
1. **Current folder:** the folder a shell last reported with OSC 7, else the current folder of the
   shell's process as the system reports it (`/proc/<pid>/cwd` on Linux, `lsof` on macOS), else the
   folder the terminal started in. The process is asked at most once a second. [STD, GEN]
2. **Links:** a relative file path is resolved against the current folder; when no file exists
   there, against the start folder as before (SPEC-0153). Absolute and `~/` paths are unchanged. [GEN]
3. **MCP:** `terminal_render` returns `reported_dir`, the folder of the last OSC 7 in the output. [ADR-0008]

## Acceptance criteria
- **AK-1** OSC 7 with host and percent escapes is decoded; no OSC 7, no folder (`screen::tests::osc7_reports_the_current_folder`).
- **AK-2** A real shell that ran `cd` reports the new folder without OSC 7 (`pty::current_folder_follows_cd`).
- **AK-3** MCP `terminal_render` returns `reported_dir` (`terminal_render_tool`).
- **AK-4** Hand test (round 4, no. 87).

## Out of scope
Setting up shell integration that sends OSC 7 (the process folder covers shells that do not);
remote shells over SSH, whose folder is on the host.
