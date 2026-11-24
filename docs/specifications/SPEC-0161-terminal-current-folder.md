# SPEC-0161: File links relative to the shell's current folder

- **Version:** 2 · **Date:** 2026-11-18 (v2: a nested shell without OSC 7 is followed; window check
  2026-11-18, gap TL1 in the F-076 row of PARITY)
- **Sources:** GEN (terminals resolve relative paths in output against the shell's current folder),
  STD (OSC 7 `file://host/path`, percent-encoded, as shells and terminals use it; POSIX
  `tcgetpgrp`: the foreground process group of a terminal), SPEC-0153
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
4. **v2 – Nested shells:** OSC 7 belongs to the process group that was in the foreground of the
   terminal when it arrived. When another process group is in the foreground now (the shell
   started `bash`, `sh` or another program that does not send OSC 7), the current folder is that
   group's leading process's folder; when that folder cannot be read, the OSC 7 folder as before.
   Once the nested program has ended and the reporting shell is in the foreground again, its OSC 7
   folder applies again. A nested shell that sends OSC 7 itself becomes the reporter. Without any
   OSC 7 the folder of the foreground group's leader comes before the shell's own process folder. Only
   processes of the terminal's own foreground group are asked (the group is read from the
   terminal itself), so no other process's folder is read. On macOS the folder is read with
   `proc_pidinfo` (`PROC_PIDVNODEPATHINFO`), `lsof` only when that fails. `terminal_render` has no
   running process and stays as in behaviour 3. [GEN, STD]

## Acceptance criteria
- **AK-1** OSC 7 with host and percent escapes is decoded; no OSC 7, no folder (`screen::tests::osc7_reports_the_current_folder`).
- **AK-2** A real shell that ran `cd` reports the new folder without OSC 7 (`pty::current_folder_follows_cd`).
- **AK-3** MCP `terminal_render` returns `reported_dir` (`terminal_render_tool`).
- **AK-4** Hand test (round 4, no. 87).
- **AK-5** v2: a shell that sent OSC 7 starts `sh`, which runs `cd ..`: the current folder is the
  parent; after `exit` it is the OSC 7 folder again (`pty::nested_shell_without_osc7_is_followed`).

## Out of scope
Setting up shell integration that sends OSC 7 (the process folder covers shells that do not);
remote shells over SSH, whose folder is on the host.
