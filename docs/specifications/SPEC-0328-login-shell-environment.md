# SPEC-0328: Login shell environment

- **Version:** 1 · **Date:** 2026-11-10
- **Sources:** GEN (a program started by the desktop gets the system's bare environment, not the one the
  user's shell builds from its start-up files; editors and IDEs ask the login shell for it once at start),
  STD (macOS: programs started by Finder, the Dock or `open` get the search path
  `/usr/bin:/bin:/usr/sbin:/sbin`; POSIX environment: `NAME=value` entries, a name has no `=`, neither part
  has a NUL; `env -0` ends each entry with NUL; shells: `-l` login, `-i` interactive, `-c` command; dynamic
  loaders read `DYLD_*` on macOS and `LD_*` on Linux; Rust 2024: changing the process environment is only
  sound while no other thread runs), SPEC-0042 (language servers are programs on the `PATH`), SPEC-0038
  (the terminal starts the user's shell as a login shell), SPEC-0278 (nothing of an untrusted project runs),
  SPEC-0282 (start from the app bundle), ADR-0007 (no telemetry), ADR-0008 (MCP tool per feature), rule 12
  of the agent instructions (security first)
- **Feature matrix rows:** F-001

## Purpose
A window started from Finder, the Dock or `open` finds the same tools as one started from the user's
terminal: language servers, interpreters, package managers, build tools, debug adapters and everything
else that is looked up on the `PATH` or configured through environment variables.

## Behaviour
1. **Once, at the very start.** Before anything else, the process decides whether it needs the
   environment of the user's login shell (behaviours 2–3), obtains it (4–6), and sets the variables it
   takes (7) in its own environment. This happens once per process, before any thread exists, so every
   later tool lookup and every child process — language servers, run and debug, terminals, build tools,
   installers and the MCP tool calls the window serves — uses it without any change of their own. [GEN, STD]
2. **When.** Only for a command that opens the IDE window (`ferris-idea launch`, the start from the app
   bundle, `ferris-idea ui` without `--selftest`, and `ferris-idea open <exchange file>`, which starts a window
   when none runs), and only when the process does not look as if a shell started it:
   - neither `SHLVL` is set nor `TERM` is set to something other than `dumb`, or
   - `PATH` is missing, empty or exactly the one macOS gives programs it starts itself.

   Every other command (`call`, `mcp`, `serve`, `quit`, `drive`, `trust`, `--version`, the self-test, …) and
   every test keeps the environment it was started with. [GEN, STD]
3. **Switch.** The environment variable `FERRIS_IDEA_SHELL_ENV` overrides behaviour 2: `off` never asks the
   shell and changes nothing (also no fallback); `force` asks it for every command and every kind of
   start. Other values count as not set. The case of the value does not matter. [GEN]
4. **Which shell, how.** The shell is `$SHELL`, else the shell of the user's account in the account
   database, each only when it is an absolute path to an existing file; else `/bin/zsh` on macOS and
   `/bin/sh` elsewhere. It is started as an interactive login shell (`-l -i -c <command>`; `csh` and `tcsh`
   accept the login flag only alone and are started with `-i -c`) with
   - the process's environment plus `FERRIS_IDEA_RESOLVING_ENVIRONMENT=1`, so start-up files can skip slow or
     interactive parts,
   - no input (the null device), so a question in a start-up file cannot wait for an answer; its error
     output is discarded,
   - the user's home folder as the working directory, never the project folder, so nothing in a project
     that is not trusted is read or run by the shell (SPEC-0278),
   - a session of its own, so it has no terminal: a window started from a terminal with the bare `PATH`
     must not have its shell stopped for trying to take that terminal over (zsh did, in a test on
     2026-11-10, and ran into the time limit). The shell thereby leads its own process group.
     [GEN, STD, SPEC-0278]
5. **Answer and time limit.** The command prints a marker that is unique to this run, the environment
   with each entry ended by NUL (`/usr/bin/env -0`), and the marker again. Everything before the first and
   after the last marker (greetings and other output of start-up files) is ignored; an entry is split at
   its first `=`, so values keep line breaks and further `=`. The answer counts as soon as the second
   marker arrived, also when a background program of a start-up file keeps the output open. The shell has
   5 seconds. When the time is over, when it ends without both markers, when it prints more than 8 MiB or
   cannot be started, there is no answer (behaviour 8). In every case the shell's process group is killed
   and the shell reaped before Ferris IDEA goes on; after an answer the shell first gets a quarter of a
   second to end by itself. [GEN, STD]
6. **Nothing is kept.** The answer is held in memory only until the variables are set. It is not written
   to disk, not logged, not shown and not sent anywhere. [rule 12, ADR-0007]
7. **What is taken.** Every variable of the answer is set in the process with the shell's value, with
   these exceptions, which are never taken:
   - `DYLD_*` and `LD_*`: they would change which code Ferris IDEA itself loads;
   - `BASH_FUNC_*`: functions exported by bash, code rather than settings;
   - what describes the asked shell and not the user's setup: `_`, `PWD`, `OLDPWD`, `SHLVL`, `TERM`,
     `TERMCAP`, `COLUMNS`, `LINES`, and `FERRIS_IDEA_RESOLVING_ENVIRONMENT`;
   - entries without a name, or whose name is not text.

   Variables the process has and the shell does not set stay as they are; nothing is removed. The whole
   environment is taken rather than a list of known names, because tools are configured through
   variables nobody can list in advance (`JAVA_HOME`, `GOPATH`, `NVM_DIR`, `PYENV_ROOT`, `SSH_AUTH_SOCK`,
   proxies, registries), and it is the same environment the user's own terminal and the terminal tool
   window (SPEC-0038) already have. [GEN, rule 12]
8. **Fallback.** Without an answer the inherited environment stays, and those of `/opt/homebrew/bin`,
   `/usr/local/bin`, `~/.local/bin` and `~/.cargo/bin` that exist and are not on the `PATH` yet are
   appended to it in this order (after the existing entries, so nothing the system provides is
   shadowed). [GEN]
9. **Visible outcome, without values.** The outcome has a source — `inherited` (as started), `login_shell`
   or `fallback` —, the time asking took, and a note in words that names the shell's program name and, for
   the fallback, the reason and the folders added. No value of a variable appears in any of it.
   - After asking the shell, one line is appended to `environment.log` in the Ferris IDEA log folder
     (SPEC-0270 behaviour 3): time, source, duration in milliseconds, the number of `PATH` entries and the
     note.
   - For the fallback, the note is also written to the error output, and the first window shows it in
     its status bar ("Tools may be missing: …").
   - The MCP tool `environment_source` (no arguments, no right needed) returns `source`, `duration_ms`,
     `note` and `path`, the folders of the process's `PATH` in order — nothing else. Served by a window
     it describes the window's process. [GEN, ADR-0008, rule 12]
10. **Systems.** macOS and Linux behave the same (on Linux the condition of behaviour 2 covers starts from
    a desktop launcher). On Windows nothing is asked and nothing changes: programs get the user's
    environment from the system there. [GEN]

## Acceptance criteria
- **AK-1** The decision: a window with the bare macOS `PATH`, or without `SHLVL` and a real `TERM`, asks
  the shell; a window started from a shell does not; a command that opens no window does not; `off`
  never asks and `force` always does (unit tests).
- **AK-2** Reading the answer: output before the first and after the last marker is ignored, entries end
  at NUL, values keep line breaks and `=`, entries without a name or without `=` are dropped, and an
  answer without the second marker is none (unit test).
- **AK-3** The filter: `DYLD_*`, `LD_*`, `BASH_FUNC_*`, the shell-state names and
  `FERRIS_IDEA_RESOLVING_ENVIRONMENT` are not taken; other names are; variables only the process has are
  neither set again nor removed (unit tests).
- **AK-4** The fallback appends only the tool folders that exist, each once, after the existing entries,
  and starts from the system's default when there is no `PATH` (unit test).
- **AK-5** With a script as the shell that prints noise, then the marker and an environment with a temp
  folder at the front of `PATH`: the source is `login_shell`, a program in that folder is found on the
  resulting `PATH`, a value with a line break arrives whole, no loader variable is set, the shell was
  started with `-l -i -c`, in the home folder, without input and with
  `FERRIS_IDEA_RESOLVING_ENVIRONMENT=1`, and the note holds no value (integration test).
- **AK-6** With a shell that sleeps past the time limit: the source is `fallback`, only `PATH` changes
  (the existing tool folder is appended), the note names the reason and the folder, and the shell and
  its child process are gone (integration test).
- **AK-7** An answer counts although a background program keeps the output open, and that program is
  gone afterwards; a shell that ends without an answer gives the fallback (integration tests).
- **AK-8** The shell is not started for a process started from a shell, with `off`, or without a window;
  with `force` it is (integration test).
- **AK-9** `environment_source` returns exactly `source`, `duration_ms`, `note` and `path` (protocol test).
- **AK-10** Through the binary: `ferris-idea call … environment_source` with `FERRIS_IDEA_SHELL_ENV=force`, a
  bare `PATH` and a stub shell reports `login_shell` and the stub's folder on `path`, and a tool call of
  the same process finds a program in that folder; the log line and all output hold no value the stub
  exported; without the switch the same command keeps its environment and does not start the shell
  (binary tests). The command lines that open a window are told from the others (unit test).
- **AK-11** Hand test: start `FerrisIDEA.app` from Finder with a Python project; the status bar names a
  running Python language server, and `environment_source` over the window's MCP endpoint says
  `login_shell`.

## Out of scope
Refreshing the environment while the window runs (a change in a start-up file needs a restart); a
settings page for the shell or the time limit; per-project environments (`.envrc`, `.env` files, version
managers that switch by folder — the shell is asked in the home folder); the environment of
`ferris-idea serve` on a remote host (SPEC-0158); shells that do not accept `-l -i -c` or have no `printf`
(they give the fallback).

## Known deviations from the reference
None claimed: how the reference obtains its environment was not observed. Own decisions, security
first: loader variables and exported shell functions are never taken, the shell never runs in the
project folder, and no value is logged, shown or sent.
