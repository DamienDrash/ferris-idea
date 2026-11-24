# Windows port: inventory of what is Unix-only

- **Status:** inventory for [ADR-0036](../adr/0036-windows-support.md) (proposed) · **Date:**
  2026-11-18 · **Requested by:** the Project Owner (2026-11-18, "prepare Windows") · **Tracking:**
  ROADMAP M6.14 (backends on every host) and the milestones proposed in ADR-0036
- **Scope:** every crate of the workspace and `tests/`, at `main` @ `e6cd4d02`. Nothing was
  downloaded for it; no Windows SDK, compiler or image was installed.

Ferris IDEA has never been built for Windows as a whole. Only the headless backend `ferris-idea-serve`
is cross-compiled for `x86_64-pc-windows-gnu` and `aarch64-pc-windows-gnullvm`, without the feature
`mcp`, and has never run on Windows (SPEC-0321 behaviour 2, `scripts/build-backends.sh`). This page
lists what stands between the workspace and a Windows build that is as secure as the macOS and
Linux builds, grouped by area, with the proposed Windows mechanism and a size (S < M < L, as in
[REMOTE_AND_MOBILE.md](REMOTE_AND_MOBILE.md)).

**Rule for every row (rule 12, "security first"):** a Windows mechanism must be equivalent to or
stronger than what macOS and Linux do. Where Unix code sets mode 0600 or 0700, Windows code sets an
owner-only access control list and checks it; where Unix code checks a peer's user id, Windows code
checks the peer's user SID. Today 48 `#[cfg(not(unix))]` fallbacks exist; some fail closed (good),
some silently drop the protection (listed under [Weak fallbacks](#weak-fallbacks-today)). A Windows
build must not ship with any of the second kind.

## How it was found

A search over `crates/` and `tests/` (Rust sources) for platform switches and Unix interfaces:

| Pattern | Files | Lines |
|---|---:|---:|
| `std::os::unix` / `os::unix::` | 106 | 214 |
| … of which `os::unix::fs::symlink` (almost all in tests) | — | 72 |
| `#[cfg(unix)]` and `cfg(all(test, unix))` | — | 225 |
| `#[cfg(not(unix))]` fallbacks | 33 | 48 |
| `cfg(target_os = "macos" / "linux")` in `workbench` / `app` | 40 / 6 | 190 / 163 |
| file modes `0o600` / `0o700` | 43 | 106 |
| `killpg` / `signal::kill` | 11 | — |
| `process_group(0)` / `setsid` | 10 | — |
| Unix sockets (`UnixStream`, `UnixListener`) | 7 | — |
| `"-c"` (shell command lines) | 57 | — |
| `"/tmp…"` literals (mostly tests) | 38 | — |
| `sshd`-based tests | 14 | — |
| X11 helpers (`x11rb`, XTEST) | 4 | — |
| `cfg(windows)` today | 2 | 2 |

Dependencies with a Unix-only target section: `nix` 0.28 in `core`, `app`, `pairing`, `terminal`,
`host`, `session`, `debugger`, `vcs` (and `remote` as a dev-dependency without a target section,
which breaks a Windows test build); `libc` in `pairing`; `rustix` (termios) in `containers`;
`x11rb` in `workbench` and `app` (Linux). `gpui` is a dependency of `ui`, `workbench` and `app` only
for `macos` and `linux`, so on Windows those crates lose the toolkit entirely.

### Compile check without a Windows SDK (2026-11-18)

The target `x86_64-pc-windows-gnu` was already installed in the Rust toolchain on the Mac, so
`cargo check --workspace --target x86_64-pc-windows-gnu --keep-going` ran without any download. No
MinGW C compiler is installed, so every crate with C code stopped in its build script; the result is
still a useful map:

| Result | Crates |
|---|---|
| Checks clean (warnings only: unused `Instant` in `core::shell_env`, unused `ids` in `core::run`, two in `debugger`) | `core`, `debugger`, `editor`, `http`, `plugins`, `profiler`, `refactoring`, `terminal`, `uidriver` |
| Rust errors | `session`: `reattach::write_private` exists only under `cfg(unix)` but `room/bind.rs` and `docs.rs` call it everywhere (2 errors). `ui`: the tooltip view uses `gpui`, which is not a dependency on Windows (6 errors) |
| Stopped at C code (needs a C compiler for the target) | `tree-sitter` and its 27 grammars (`languages` and everything above it), `ring` (rustls in `web`, `plugins`, `database`), `libsqlite3-sys`, `libduckdb-sys` (C++), `odpic-sys` |

Not reached because of the above: `languages`, `database`, `nosql`, `host`, `remote`, `mcp`,
`session`'s dependants, `serve` (with `mcp`), `workbench`, `app`, `web`, `pairing`, `containers`,
`vcs`, `ai`, `bench`, `hardening`. The cross build in `scripts/build-backends.sh` runs in a Docker
container with MinGW and gets further (`ferris-idea-serve` without `mcp` builds there).

## Inventory by area

### 1. Build, toolchain and C dependencies — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| C/C++ code in build scripts: tree-sitter (28 crates), `ring`, SQLite, DuckDB, ODPI-C | `languages`, `web`, `plugins`, `database` | MSVC (`x86_64-pc-windows-msvc`) builds all of them; DuckDB with MinGW failed before (SPEC-0321, PO-STATUS 2026-11-09), so MSVC is the primary target | none beyond pinned versions |
| `gpui` only for `macos`/`linux` | `ui`, `workbench`, `app` `Cargo.toml` | add a `cfg(target_os = "windows")` section with `gpui`'s Windows backend (DirectX 11, DirectWrite) | — |
| `gpui` release shaders | `vendor/gpui/build.rs` | release builds compile HLSL with `fxc.exe` from the Windows SDK, and only when the **build host** is Windows; debug builds compile the shaders at run time from the source path. A release build therefore needs a Windows build machine (or a pre-built `shaders_bytes.rs`, which would be a fourth vendored change) | the shader blob must come from our build, not from a download |
| Windows manifest and resources (DPI awareness, long paths, icon) | `vendor/gpui` feature `windows-manifest` (`embed-resource`) | `rc.exe` (Windows SDK) or `llvm-rc`; declare `longPathAware` and per-monitor DPI v2; use `packaging/icons/windows/ferris-idea.ico` | — |
| `nix` as an unconditional dev-dependency | `crates/remote/Cargo.toml` | move under `[target.'cfg(unix)'.dev-dependencies]` | — |
| `session::reattach::write_private` missing off Unix | `session` | one private-file module for both systems (area 4) | today the Windows path would write without protection |

### 2. Terminal → ConPTY — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Pseudo terminal | `terminal/src/session.rs`, `host/src/term.rs` (`portable-pty` 0.9) | `portable-pty` already uses ConPTY on Windows 10 1809+; keep it | — |
| Default shell from `$SHELL`, else `/bin/sh` | `terminal/src/session.rs:126`, `host/src/term.rs:167` | PowerShell 7 (`pwsh.exe`) if found, else Windows PowerShell, else `%ComSpec%`; a setting picks Git Bash, WSL or another shell | resolve the shell to an absolute path (area 10) |
| Shell integration scripts for zsh and bash | `terminal/src/integration.rs` | a PowerShell prompt function that writes the same OSC 133 marks | the script is written to a private folder (area 4) |
| Ending the shell and its children with `killpg` | `terminal/src/session.rs:362`, `host/src/term.rs` | close the pseudo console and end a Job Object that holds the shell (area 5) | — |
| Raw terminal for `docker exec` (`rustix::termios`) | `containers/src/exec.rs` | Console API `SetConsoleMode` with virtual terminal input/processing | — |

### 3. Secrets → Windows Credential Manager — size S (code) + a decision

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Database and NoSQL passwords, AI keys | `database/src/secret.rs`, `nosql`, `ai`, `mcp` (`keyring` 3.6) | `keyring` is already built with `windows-native`: Windows Credential Manager, encrypted with DPAPI for the user | Credential Manager has **no per-program access list**: any program of the same user can read an entry. macOS's keychain asks before another program reads; Linux's Secret Service behaves like Windows. This is a deviation the Project Owner decides (ADR-0036, D1). Entries are limited to 2,560 bytes; the values stored today (passwords, 32-byte keys) fit |
| Pairing keys | `pairing/src/keys.rs` | keychain store through `keyring` as on macOS; the file store is refused off Unix (`cfg(not(unix))` returns an error) — keep it so | fails closed today: good |
| The file store for hosts without a keychain | `database/src/secret.rs:596–650` | stays refused on Windows (every Windows desktop has Credential Manager) | fails closed today: good |
| Agent vault (ADR-0033, proposed) | — | its key in Credential Manager, optionally wrapped with DPAPI-NG or the TPM later | same per-program caveat |

### 4. File permissions → owner-only ACLs — size L

Unix code creates private folders with 0700 and private files with 0600 (106 lines in 43 files) and,
in some places, refuses a file that others can read. Windows has no modes; `std` only offers
"read-only".

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Private folders and files: MCP discovery token, reattach folder, UI-driver trace, query history, Java debug folder, Delve folder, browser debug profile, remote recent list, SSH control folder, TLS key, pairing and secret file stores | `mcp/src/http.rs:214,361`, `session/src/reattach.rs`, `session/src/replica.rs:416`, `uidriver/src/trace.rs`, `database/src/history.rs`, `languages/src/java_debug.rs`, `debugger/src/delve.rs`, `debugger/src/browser.rs`, `remote/src/recent.rs`, `remote/src/ssh.rs`, `web/src/tls.rs`, `workbench/src/workbench_remote_project.rs:177` | one module (proposed `ferris_idea_core::private_fs`): `create_private_dir`, `write_private` (temporary file + rename), `append_private`, `check_private`. On Windows it sets a **protected** DACL (no inherited entries) with one entry: the current user's SID, full control; checking reads the DACL and refuses a NULL DACL, an unprotected DACL or any entry for another SID | equal to 0600/0700. Administrators and `SYSTEM` can still read through backup privileges, as `root` can on Unix |
| "Readable by others" checks | `web/src/tls.rs` (`check_key_permissions`), `debugger/src/browser.rs` (`tighten`), `remote/src/testing.rs` | `check_private` as above | today these are `Ok(())` off Unix — **weaker**, see below |
| Executable bit (run configurations, Git file modes, language-server detection) | `core/src/run_edit.rs`, `vcs/src/git.rs:2151–2176`, `languages/src/servers.rs:357`, `vcs/src/hooks.rs:209` | executable = extension in `PATHEXT`; Git keeps the mode from the index (`core.fileMode=false` as Git for Windows does) | — |
| File properties dialog shows the Unix mode | `core/src/file_properties.rs`, `session/src/fsops.rs:80` | show read-only and the owner; never offer `chmod` | — |

### 5. Processes and signals → Job Objects and console control events — size L

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Start in a new process group, stop with SIGTERM then SIGKILL to the group | `core/src/run.rs` (`STOP_GRACE`), `vcs/src/{hooks,clone,remote}.rs`, `debugger/src/{client,lifetime,adapters,delve}.rs`, `host/src/term.rs`, `terminal/src/session.rs` | `CREATE_NEW_PROCESS_GROUP` plus a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; graceful stop sends `CTRL_BREAK_EVENT` to console programs, `WM_CLOSE` to windowed ones, then `TerminateJobObject` after the grace period | a Job Object also guarantees nothing outlives Ferris IDEA, which the process group does not |
| Exit codes "128 + signal" | `core/src/run.rs:937` | report the `NTSTATUS` (for example `0xC000013A` after Ctrl+Break) with its name | — |
| Is a process alive (`kill(pid, 0)`) | `mcp/src/client.rs:72`, `mcp/src/http.rs:403` | `OpenProcess` + `GetExitCodeProcess`, compared with the start time so a reused PID is not taken for the old one | today `true` off Unix: a stale discovery file is never cleaned |
| Detach the window from the launching terminal (`setsid`) | `app/src/main.rs` | `DETACHED_PROCESS` / `CREATE_NO_WINDOW`; a `windows_subsystem = "windows"` binary for the window | — |
| Login-shell environment | `core/src/shell_env.rs` | not needed: Windows programs inherit the user's environment from the registry; the `cfg(not(unix))` path already returns "inherited" | — |

### 6. Sandboxing → AppContainer / Job Objects — size S today

No Seatbelt, Landlock or other OS sandbox exists in the workspace (search for `sandbox-exec`,
`Seatbelt`, `landlock`: no hits). Plugins run in WebAssembly (ADR-0016), which is portable.
`ai/src/subscription.rs` passes `--sandbox read-only` to the Codex CLI, whose own Windows sandbox is
the provider's and is not verified. If an OS sandbox for agent runs is added later, the Windows
counterpart is an AppContainer or a restricted token inside a Job Object (ADR-0036, phase 5).

### 7. File watching — size S (works), M (native)

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Watching by snapshots, renames paired by inode | `core/src/watch.rs` (portable polling, no `notify`) | works as is; the inode is `None` off Unix, so a rename shows as delete + create (a test covers this). Add the NTFS file index (`GetFileInformationByHandle`) as the inode | — |
| Local history file identity | `core/src/history.rs:112` (returns `0` off Unix) | the same file index | — |
| Native events (later) | — | `ReadDirectoryChangesW` (through the `notify` crate) as an optimisation; polling stays the fallback for network drives | — |

### 8. Paths, folders and file names — size L

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Path text built and split with `/` (164 files) | throughout, mostly `workbench`, `languages`, `core`, `mcp` | `Path`/`PathBuf` for disk paths; `/` only for workspace-relative paths in protocols and settings, converted at the edge | — |
| `canonicalize` returns `\\?\C:\…` | `core/src/project_root.rs`, `trust.rs`, `own_sources.rs`, `agent_files.rs`, `session/src/paths.rs` | strip the verbatim prefix where possible (`dunce`-style) before comparing or showing | a prefix check between `\\?\C:\x` and `C:\x` fails open or closed depending on direction: every "inside the project" check gets a Windows test |
| Case-insensitive, case-preserving names | 73 files compare or lower-case paths | compare paths with the file system's rules (`CompareStringOrdinal` ignore-case) for "same file" and "inside folder" checks | an agent must not escape a folder by writing `..\SRC` |
| Links: symlinks need a privilege or Developer Mode; junctions and other reparse points do not | 72 test lines create symlinks; `own_sources.rs` opens with `O_NOFOLLOW` | treat every reparse point (symlink, junction, mount point) as a link in escape checks; open with `FILE_FLAG_OPEN_REPARSE_POINT`; tests create junctions or skip with a reason | junctions are the Windows way out of a project folder and must be checked like symlinks |
| Reserved names (`CON`, `NUL`, `AUX`, `COM1`…), trailing dots and spaces, `:` (alternate data streams), length over 260 | file creation, rename, refactoring, Git checkout | refuse such names with a clear message; long paths through the manifest | `file.txt:secret` writes a hidden stream: refuse `:` in names |
| Settings, caches, logs, runtime folders from `HOME`, `XDG_*`, `~/Library` | `core/src/permissions.rs:374`, `database/src/connector.rs:1246`, `remote/src/mirror.rs`, `session/src/reattach.rs:115`; `core/src/logs.rs` already uses `%LOCALAPPDATA%\FerrisIDEA\Logs` | `%APPDATA%\FerrisIDEA` (settings), `%LOCALAPPDATA%\FerrisIDEA` (caches, logs, runtime) via `SHGetKnownFolderPath`; `HOME` is not set on Windows | the folders get the owner-only ACL of area 4 |
| Line endings and code pages | editor (CRLF is handled), process output | keep CRLF as found; decode program output as UTF-8 and set `CP_UTF8` for child consoles; name the OEM code page when output is not UTF-8 | — |

### 9. Shell invocations — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| `sh -c` and `/bin/sh` (57 files, many in tests) | `vcs/src/hooks.rs`, `debugger/src/lifetime.rs`, `debugger/src/client.rs`, `containers/src/ports.rs`, `host/src/dap.rs`, `bench`, run configurations | start programs directly with an argument list wherever possible; for a user's shell command line use PowerShell (`-NoProfile -NonInteractive -EncodedCommand`) | never build a `cmd.exe /C` line from untrusted text; Rust ≥ 1.77.2 escapes arguments for `.bat`/`.cmd` or refuses (CVE-2024-24576) — keep the toolchain at least there |
| Git hooks run through `/bin/sh` | `vcs/src/hooks.rs:104–121` | Git for Windows runs hooks with its own `sh.exe`; use `git hook run <name>` (Git ≥ 2.36) instead of starting the script | — |
| Fake programs in tests are shell scripts | `debugger`, `languages`, `vcs`, `app`, `tests/functional` | small Rust helper binaries or `.cmd` files in the test fixtures | — |

### 10. Program lookup and launches of `.exe`/`.cmd` (LSP, DAP, build tools) — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Ten separate `on_path`/`which` functions, none honours `PATHEXT` | `core/src/python_index.rs:555`, `app/src/workbench.rs:13797`, `languages/src/python_tools.rs:99`, `languages/src/debug_plan.rs:300`, `debugger/src/adapters.rs:21`, `profiler/src/run.rs:127`, `containers/src/compose.rs:238`, `remote/src/pq.rs:172` (and two in tests) | one lookup (`ferris_idea_core::program::find`) that walks `PATH` × `PATHEXT` and returns an absolute path | **never search the project folder or the current directory** (binary planting); a program found inside the project needs trust (SPEC on trusted projects) |
| npm-installed servers are `.cmd` shims: `typescript-language-server`, `pyright-langserver`, `npx`, `vscode-*` | `languages/src/servers.rs`, `debugger/src/adapters.rs` | resolve the `.cmd` and start it through the escaping of area 9, or start `node.exe` with the package's script directly (preferred: no batch file) | as area 9 |
| Python virtual environments: `bin/python` | 11 files | `Scripts\python.exe` | — |
| `gradlew` vs `gradlew.bat`, `mvn` vs `mvn.cmd`, `java.exe`, `dlv.exe`, `lldb-dap.exe`, rustup proxies | `core` (JVM, build tools), `languages/src/servers.rs:374` (`rustup_proxy` returns `None` off Unix) | the lookup above; `rustup which` for proxies | — |

### 11. Local sockets → named pipes — size L

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Reattach socket of the backend (peer uid checked) | `session/src/backend.rs:1551`, `session/src/reattach.rs`, `serve/src/lib.rs:171` | a named pipe `\\.\pipe\ferris-idea-<user-sid>-<id>` with a security descriptor for the current user only, `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE`; the server checks the client's token user SID (`GetNamedPipeClientProcessId` → `OpenProcessToken`) | equal to the peer-uid check; today Windows has no reattach at all (SPEC-0321 behaviour 8) |
| Debug adapters over Unix sockets in a 0700 folder | `host/src/dap.rs:361–432`, `debugger/src/client.rs`, `debugger/src/delve.rs`, `languages/src/java_debug.rs` | named pipes as above where the adapter supports them; otherwise loopback TCP **with** a per-start secret the adapter must present | Java debug falls back to a plain loopback port off Unix today (PO-STATUS 2026-11-09): another local user could connect — weaker, must not ship |
| Docker Engine socket | `containers/src/docker.rs:470` | Docker Desktop's pipe `\\.\pipe\docker_engine` | the pipe's own ACL (Docker users group) applies |
| `ssh` connection sharing (`ControlMaster`) | `containers/src/docker.rs:447`, `remote/src/ssh.rs:465` | Windows OpenSSH does not support `ControlMaster`; one connection per use (already the case for `remote` off Unix) | — |

### 12. Window, menus, fonts and input (GPUI) — size L

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| Window backend | `vendor/gpui/src/platform/windows/` (DirectX 11, DirectWrite, `windows` crate) | use it; the vendored changes already keep `PromptButton::Safe` exhaustive there (FERRIS_IDEA.md change 2). The focus rule for prompts (change 2) must be ported to the Windows task dialog, or the drawn prompt of change 3 used on Windows too | the safe-answer rule (SPEC-0281) must hold on Windows before release |
| macOS/Linux switches in the views | `workbench` (55 `linux`, 135 `macos` in 40 files), `app` (25 / 138 in 6 files) | review each `target_os = "linux"`: most mean "PC keyboard and drawn menu bar" and become `not(target_os = "macos")`; X11-only code stays `linux` | — |
| Menu bar | `workbench/src/menu_bar.rs`, `workbench_menus.rs` | the menu bar Ferris IDEA draws on Linux, as on Windows the reference has one in the window | — |
| Keymap | `ui/keymaps`, `workbench/src/keymaps.rs` | the bundled default keymap (written for Linux) is the reference's Windows/Linux keymap; check the keys Windows reserves (Win key, Alt+F4, Ctrl+Alt+Del, AltGr = Ctrl+Alt on German layouts) | — |
| Fonts | `workbench/src/fonts.rs` (JetBrains Mono bundled), `run_view.rs:915`, `debug_view.rs:2040`, `core/src/ide_settings.rs:1088` (Menlo/DejaVu Sans Mono by system) | bundled font as now; system choices Cascadia Mono/Consolas, UI font Segoe UI | — |
| In-process input for the window self-test | `app/src/native_click.rs`, `workbench/src/x11_wake.rs` (X11) | gpui's pointer hook (change 1) is platform-neutral; X11 wake-up is not needed on Windows | — |
| Accessibility | ADR-0019, ADR-0027 phase 4 | AccessKit's UI Automation adapter | — |

### 13. Profiler — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| CPU sampling of native programs with `/usr/bin/sample` (macOS) or `perf` (Linux) | `profiler/src/{sample,perf,run,attach}.rs`, `mcp` tool description | ETW sampling, through the Windows Performance Recorder (`wpr.exe`, part of Windows) or an open-source ETW sampler; Python cProfile and JFR work unchanged | ETW sampling of other users' processes needs administrator rights; Ferris IDEA never asks to elevate |
| Memory with `leaks`/`malloc_history` or Valgrind Massif | `profiler/src/memory.rs`, `run.rs` | none built in at first: refused with a reason (as Java memory is today); later ETW heap tracing | — |

### 14. Remote work from a Windows window — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| System OpenSSH client | `remote/src/ssh.rs`, `remote/src/pq.rs` | Windows 11 ships OpenSSH 9.5, which has `sntrup761x25519-sha512`; ADR-0025 §1a's key-exchange list holds; `ssh.exe` is found with the lookup of area 10 | refuse a client without a post-quantum key exchange, as on macOS |
| WSL transport | `remote` (`wsl://`) | exists, never run on Windows | — |
| Git-over-SSH helper is a POSIX script | `vcs` | a small Rust helper or a `.cmd` | — |
| Listeners on 127.0.0.1 (56 files) | `mcp`, `serve`, `web`, debuggers | no firewall prompt for loopback; `--allow-network` triggers the Windows Defender Firewall prompt, which Ferris IDEA explains before | other local users can reach loopback ports on Windows as on Unix: the existing tokens stay mandatory |

### 15. Installer, updates, signing — size M

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| No installer | `packaging/` has only `icons/windows/ferris-idea.ico` | ZIP for test builds; MSI (WiX) or MSIX for releases (ADR-0036 decision 6) | per-user install below `%LOCALAPPDATA%\Programs` needs no administrator rights |
| No updater | no self-update code in the workspace (ADR-0007: nothing calls home) | none in the product; updates through the installer or `winget` later | — |
| Code signing | `scripts/bundle-macos.sh` signs ad hoc only | Authenticode; unsigned test builds are marked as such | SmartScreen warns on unsigned programs; costs money, the Project Owner decides |

### 16. Tests and CI — size L

| What | Where | Windows mechanism | Security note |
|---|---|---|---|
| `sshd`-based tests (14 files) | `remote/tests`, `database/tests/it/tunnel.rs`, `app/tests/it/remote_*`, `containers/tests` | skipped on Windows with a reason at first; later the OpenSSH Server feature in the test VM | the test VM's sshd listens only on its private network |
| X11 window tests | `app/src/native_click.rs`, `workbench/tests/it/fallback_prompt_keys.rs`, `uidriver/tests/it/scans.rs` | Linux only; Windows window tests run on a real desktop session in the VM | — |
| `/tmp` literals (38 files) | mostly tests | `tempfile` everywhere | — |
| Window self-test (`ci-window.sh`) | `scripts/` | a Windows run in the VM once the window builds | — |
| CI | `.github/workflows/ci.yml` has `windows-latest` and `windows-11-arm` jobs for `ferris-idea-serve` only, behind `HOSTED_RUNNERS` (off) | ADR-0036 decision 5 | — |

## Weak fallbacks today

These `cfg(not(unix))` branches compile on Windows and silently give less protection than on Unix.
None may remain in a Windows build; a scan test should fail on a `cfg(not(unix))` branch that
returns success without doing the work.

| Place | What it does off Unix | Must do |
|---|---|---|
| `web/src/tls.rs:104` `check_key_permissions` | accepts any TLS key file | refuse a key others can read (owner-only ACL check) |
| `debugger/src/browser.rs:198` `tighten` | leaves the browser debug profile open | owner-only ACL |
| `session/src/reattach.rs:61,68` `private_dir`, `append_private` | plain folder and file | owner-only ACL |
| `session/src/replica.rs:416`, `workbench/src/workbench_remote_project.rs:177` | plain folder for remote replicas | owner-only ACL |
| `remote/src/recent.rs:70` | plain file for the recent hosts list | `write_private` |
| `remote/src/ssh.rs:457` | ignores the requested mode | owner-only ACL |
| `mcp/src/http.rs:214,361` (`cfg(unix)` only) | discovery file with the token and its folder without protection | owner-only ACL |
| `mcp/src/client.rs:72`, `mcp/src/http.rs:403` `alive` | every PID is alive | real check (area 5) |
| `languages/src/java_debug.rs:210`, `host/src/dap.rs:432` | Java debug (jdtls) and the JavaScript debugger (js-debug) on an open loopback port | named pipe or secret (area 11) |

Fallbacks that fail closed and may stay: the file stores of `database/src/secret.rs` and
`pairing/src/keys.rs` (refused off Unix), `host/src/dap.rs:503` (no Unix socket: error).

## Biggest items

1. **Owner-only ACL module and removal of the weak fallbacks** (areas 4, 11; L) — the security
   precondition for any Windows build.
2. **Process control with Job Objects** (area 5; L) — every run, debug session, hook and terminal.
3. **Paths** (area 8; L) — verbatim prefixes, case, reparse points and reserved names, with tests
   for every "inside the project" check.
4. **GPUI Windows backend in the views** (area 12; L) — 353 OS switches to review in `workbench`
   and `app`; release shaders need a Windows build machine.
5. **Tests on Windows** (area 16; L) — fake programs as shell scripts, symlinks, `sshd`.
