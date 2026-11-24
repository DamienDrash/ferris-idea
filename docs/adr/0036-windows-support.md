# ADR-0036: Windows support — MSVC first, owner-only ACLs, phased from compile to installer

- **Status:** proposed · **Version:** 1 · **Date:** 2026-11-18 · written on the Project Owner's
  request of 2026-11-18 ("prepare Windows"). Preparation only: nothing was downloaded, no Microsoft
  licence was accepted, no Windows image was obtained. Rule 12 ("security first") applies
  throughout; every option that is weaker, costs money or needs a licence is named as the Project
  Owner's decision (open questions at the end).
- **Related:** [Windows port inventory](../architecture/WINDOWS_PORT.md) (the findings this ADR
  decides on), [ADR-0003](0003-ui-toolkit.md) (GPUI), [ADR-0007](0007-no-restrictions.md) (nothing
  calls home), [ADR-0016](0016-plugins-as-wasm.md) (plugins in WebAssembly),
  [ADR-0019](0019-accessibility-path-for-gpui.md), [ADR-0025](0025-network-security-for-remote-connections.md)
  (remote paths), [ADR-0027](0027-ui-driver-and-semantic-tree.md),
  [ADR-0030](0030-ci-strategy.md) (CI), [SPEC-0321](../specifications/SPEC-0321-every-host.md)
  (backends on every host), ROADMAP M6.14

## Context

Ferris IDEA runs on macOS and Linux. Windows has been "later" since the start (PO-STATUS platforms
line). What exists for Windows today:

- `ferris-idea-serve` (headless backend) is cross-compiled for `x86_64-pc-windows-gnu` and
  `aarch64-pc-windows-gnullvm` in a Docker container with MinGW, without the feature `mcp` because
  DuckDB does not build with MinGW. It has never run on Windows (SPEC-0321).
- Remote work already knows Windows *hosts*: PowerShell bootstrap, `%LOCALAPPDATA%` install path,
  `wsl://` transport (SPEC-0321), none of it run.
- GPUI 0.2.2, vendored, has a Windows backend (DirectX 11, DirectWrite); Ferris IDEA never built it.
- `keyring` is already compiled with `windows-native` (Credential Manager).

The inventory found 214 lines of `std::os::unix` in 106 files, 225 `cfg(unix)` switches, 48
`cfg(not(unix))` fallbacks of which about a dozen silently drop a protection, 353 macOS/Linux
switches in the window crates, ten separate program lookups without `PATHEXT`, Unix sockets in seven
files and process groups with signals in eleven. A `cargo check` for `x86_64-pc-windows-gnu` (target
already installed, no download) checked nine crates clean, found two Rust errors (`session`, `ui`)
and stopped in every crate with C code for lack of a C compiler.

## Decision

### 1. Targets

1. **`x86_64-pc-windows-msvc` first** for the window and for `ferris-idea-serve`. MSVC is the target
   Windows programs and C/C++ libraries are built and tested for; DuckDB and ODPI-C build with it,
   and GPUI's Windows backend is developed against it.
2. **`aarch64-pc-windows-msvc` second**, once phase 3 passes on x86_64 (Windows on ARM laptops).
3. The existing GNU cross builds of `ferris-idea-serve` (`x86_64-pc-windows-gnu`,
   `aarch64-pc-windows-gnullvm`) stay as a cheap compile check until the MSVC lane of decision 5
   runs, then backends move to MSVC too, so one Windows ABI is shipped and tested.
4. Minimum system: Windows 10 22H2 and Windows 11 (ConPTY, `AF_UNIX` not used, long paths through the
   manifest). Windows Server is not a target for the window; `ferris-idea-serve` on Windows Server is
   best effort.

### 2. Toolchain

Two ways to get MSVC, both use Microsoft's own compiler, linker, C runtime and Windows SDK, and both
require accepting Microsoft's licence terms — **the Project Owner's consent, not an agent's**:

| | (a) Official tools on a Windows machine | (b) `cargo-xwin` cross-compile from macOS/Linux |
|---|---|---|
| What is installed | Visual Studio Build Tools (MSVC, Windows SDK) on the Windows VM or runner | `cargo-xwin` downloads the MSVC CRT and Windows SDK files from Microsoft's servers into a cache; `clang-cl`/`lld-link` from LLVM do the work |
| Licence | Visual Studio licence terms (which edition applies to this project is the Project Owner's call) and Windows SDK terms | the same Microsoft terms; `xwin` requires an explicit "accept licence" switch |
| Can build | everything, including GPUI release shaders (`fxc.exe`, needs a Windows build host) and the resource file | everything except GPUI **release** shaders (the build script compiles them only on a Windows host); debug builds and `cargo check` work |
| Can test | yes | no (needs Windows to run) |

**Decided:** (b) for compile checks on every change (fast, on the existing CI machine, no Windows
needed) and (a) on the Windows test machine for tests, release builds and installers. Both wait for
consent C1. Until then, only the already installed `x86_64-pc-windows-gnu` target is used for
`cargo check` of crates without C code.

### 3. Security rules for the port (binding for every Windows change)

1. **Owner-only ACL module.** One module (proposed `ferris_idea_core::private_fs`) replaces every
   0600/0700 and every "readable by others" check: on Windows a protected DACL with a single entry
   for the current user's SID; checks refuse NULL or unprotected DACLs and entries for other SIDs.
   It fails closed.
2. **No silent fallback.** A `cfg(not(unix))` branch that returns success without doing the work is
   forbidden; a scan test enforces it. The weak fallbacks listed in the inventory are fixed before a
   Windows build is offered to anyone.
3. **Local channels authenticate the peer.** Named pipes carry a user-only security descriptor,
   `PIPE_REJECT_REMOTE_CLIENTS` and `FILE_FLAG_FIRST_PIPE_INSTANCE`, and the server checks the
   client's user SID. A debug adapter that only speaks TCP gets a per-start secret; an open loopback
   port without one is not allowed (it is today for jdtls and js-debug off Unix).
4. **Program lookup never searches the project or the current directory**; it returns absolute paths
   and honours `PATHEXT`. Batch files (`.cmd`, `.bat`) are avoided where the real program can be
   started (for example `node.exe` with a package script); where not, only through Rust ≥ 1.77.2's
   escaping, never a hand-built `cmd.exe /C` line.
5. **Reparse points are links.** Symlinks, junctions and mount points count as links in every
   "inside the project" check; names with `:` (alternate data streams), reserved device names and
   trailing dots or spaces are refused.
6. **Processes live in Job Objects** with kill-on-close, so nothing outlives Ferris IDEA.
7. **Remote paths unchanged (ADR-0025).** The Windows OpenSSH client must offer a post-quantum key
   exchange of ADR-0025 §1a or is refused; `--allow-network` explains the firewall prompt first.
8. **Secrets in Credential Manager.** Deviation: Credential Manager has no per-program access list
   (macOS's keychain asks before another program reads; Linux's Secret Service does not). The
   Project Owner decides whether to accept this as on Linux (D1); the file stores stay
   refused on Windows.
9. **Unsigned builds are marked.** Every unsigned Windows build says so in its file name
   (`-unsigned`), in `--version` and in the About dialog, and is never published.

### 4. Test environment

| Option | Fits | Costs and consents |
|---|---|---|
| **(a) Windows 11 ARM in a VM on the Mac (UTM)** | functional tests, window tests on a real desktop, installer tests; x64 builds run through Windows' own emulation (not for performance numbers) | UTM is free (download, consent C3); a Windows 11 licence or the time-limited evaluation/Insider image (Microsoft account, licence terms: consent C2); uses the Mac's memory and disk |
| **(b) A cloud VM (x64)** | native x64, performance numbers, an always-on runner | costs money: consent C4 |
| **(c) GitHub Actions `windows-latest` / `windows-11-arm`** | x64 and ARM64 tests in CI | only within the free minutes of the plan (Windows minutes count double on private repositories), never paid; turning on the hosted runners is consent C5 |

**Proposed:** (a) as the primary test machine — it is the Mac the project already uses, it keeps the
code on the Project Owner's hardware, and it covers the window. (c) on demand within free minutes
for x64 checks. (b) only if (a) proves too slow.

### 5. CI plan (extends ADR-0030)

1. **Windows compile lane** on the CI server: `cargo xwin check --workspace --target
   x86_64-pc-windows-msvc --all-targets` for every change that touches Rust code, with the result
   remembered by tree like the other lanes. Runs only after consent C1.
2. **Windows test lane** on the Windows VM: nightly, and on request for a branch, `cargo test` of the
   crates phase 2 has made Windows-ready (a list in `scripts/`), later the window self-test. A
   branch that touches `cfg(windows)` code is merged only after this lane passed (as the macOS
   window tests today).
3. **Hosted runners** stay off (`HOSTED_RUNNERS`); the existing `windows-latest`/`windows-11-arm`
   jobs for `ferris-idea-serve` run only if consent C5 is given.
4. Skips are explicit: a test that cannot run on Windows says why (`sshd`, X11, `/usr/bin/sample`),
   and the skip list is reviewed per phase.

### 6. Packaging and signing

| | MSI (WiX) | MSIX |
|---|---|---|
| Install | per user, below `%LOCALAPPDATA%\Programs\FerrisIDEA`, no administrator rights | per user, container with a package identity |
| Fits an IDE | yes: unrestricted access to tool chains, PATH, child processes | partly: file system and registry virtualisation and the package identity get in the way of spawning compilers, debuggers and shells; needs a signature to install at all |
| Tooling | WiX Toolset (licence terms of the version used must be checked by the Project Owner, consent C6) | `makeappx.exe`/`signtool.exe` from the Windows SDK (consent C1) |

**Proposed:** a ZIP for test builds (phase 2–3), a per-user **MSI** for releases (phase 4); MSIX
only if a store listing is wanted later. **Signing:** Authenticode with an OV or EV certificate or a
cloud signing service — each costs money every year (consent C7). Without it, SmartScreen warns on
every download; builds stay "unsigned, for testing" (rule 3.9). No auto-updater (ADR-0007); a
`winget` manifest may follow a signed release.

### 7. Phases

| Phase | Goal | Done when |
|---|---|---|
| **W0 Preparation** (this ADR) | inventory, decisions, consents asked | ADR accepted, consents answered |
| **W1 Compile** | the whole workspace compiles for `x86_64-pc-windows-msvc`; owner-only ACL module; weak fallbacks removed; `cfg` review of `workbench`/`app`; `nix` dev-dependency of `remote` fixed | `cargo xwin check --workspace --all-targets` green in the compile lane |
| **W2 Headless core and `ferris-idea serve`** | Job Objects, program lookup, paths, named pipes, Credential Manager, ConPTY shell, Git hooks, `ferris-idea serve`/`ferris-idea mcp`/`ferris-idea-serve` with the `mcp` feature | tests of `core`, `editor`, `languages`, `vcs`, `session`, `serve`, `mcp`, `database`, `terminal`, `debugger` pass on the Windows VM; functional tests over MCP stdio pass; security tests for each rule of decision 3 |
| **W3 Window** | GPUI Windows backend, drawn menu bar, keymap, fonts, prompts with the safe-answer rule, window self-test | the window self-test passes on the Windows VM; hand tests of a Windows round done |
| **W4 Installer** | MSI, marking of unsigned builds, `aarch64-pc-windows-msvc`, signing if decided | MSI installs and uninstalls per user without administrator rights; Windows on ARM self-test passes |
| **W5 Later** | ETW profiler, native file watching, UI Automation (ADR-0027 phase 4), OS sandbox for agent runs (AppContainer) | own specs |

### 8. Proposed ROADMAP entries (not entered, not ticked)

For ROADMAP once this ADR is accepted, as a new milestone with weight 0 (platform work, not parity):

> **M21 · Windows · Gewicht 0 · erfüllt: 0** — Plan: `docs/adr/0036-windows-support.md`,
> Inventar: `docs/architecture/WINDOWS_PORT.md`
>
> - [ ] **M21.1 Kompilieren: ganzer Workspace für `x86_64-pc-windows-msvc`, Modul für Nur-Besitzer-ACLs, keine stillen `cfg(not(unix))`-Rückfälle, Kompilier-Spur im CI**
> - [ ] **M21.2 Kopfloser Kern und `ferris-idea serve` auf Windows: Job Objects, Programmsuche mit `PATHEXT`, Pfade, Named Pipes, Credential Manager, ConPTY; Tests auf der Windows-VM**
> - [ ] **M21.3 Fenster auf Windows: GPUI-Windows-Backend, Menüleiste, Tastenbelegung, Schriften, Fenster-Selbsttest**
> - [ ] **M21.4 Installer (MSI pro Benutzer), Kennzeichnung unsignierter Builds, `aarch64-pc-windows-msvc`, Signatur falls entschieden**
> - [ ] **M21.5 Später: ETW-Profiler, natives Dateibeobachten, UI Automation, Sandbox für Agentenläufe**

M6.14 ("every host") stays as it is; it is ticked only after a run on a real Windows host, which
phase W2 provides.

## Consequences

- Every Windows change carries a security test for the rule of decision 3 it touches; the weak
  fallbacks become errors on Windows before they become code.
- The CI server gains a compile lane (minutes per change); the Windows VM adds a nightly lane on the
  Mac and needs memory and disk there.
- 353 OS switches in the window crates are reviewed once; afterwards "PC" (`not(target_os =
  "macos")`) and "X11" (`target_os = "linux"`) are distinct, which also helps FreeBSD later.
- GPUI release builds for Windows need a Windows build host because of `fxc.exe`.

## Alternatives considered

- **GNU target (MinGW) as the main target.** No Microsoft licence needed, already used for
  `ferris-idea-serve`; rejected as primary because DuckDB does not build with it, GPUI's Windows
  backend is developed against MSVC, and debuggers and profilers on Windows expect PDB files.
- **Port only `ferris-idea-serve` and use Windows through WSL.** Smaller; rejected as the goal because a
  Windows user expects the window, but WSL stays a supported transport.
- **`AF_UNIX` sockets on Windows instead of named pipes.** Available since Windows 10 1803, but `std`
  does not support them and there is no peer-credential check; rejected (decision 3.3).
- **MSIX as the only package.** Rejected for an IDE that must start arbitrary tool chains (decision 6).

## Open questions for the Project Owner (defaults apply until answered)

These are also PO-STATUS question 49. C-numbers are consents, D-numbers decisions.

1. **C1 Microsoft toolchain licence:** may the Visual Studio Build Tools (MSVC, Windows SDK) be
   installed on the Windows VM, and may `cargo-xwin` download the MSVC CRT and Windows SDK files on
   the CI machine, accepting Microsoft's licence terms? *Default:* no; only `cargo check` with the
   installed GNU target, crates with C code unchecked.
2. **C2 Windows image and licence:** which Windows 11 (ARM) image and licence — evaluation/Insider
   image with a Microsoft account, or a bought licence? *Default:* none; no Windows tests.
3. **C3 UTM download** (free, open source) for the VM on the Mac. *Default:* no.
4. **C4 Cloud VM** (costs money). *Default:* no.
5. **C5 GitHub-hosted Windows runners within free minutes** (turning on `HOSTED_RUNNERS` for the
   Windows jobs only). *Default:* off.
6. **C6 Installer tooling:** WiX Toolset (check the licence terms of the current version) or
   Microsoft's MSIX tools (part of C1). *Default:* ZIP only.
7. **D1 Credential Manager without per-program access list** — accept as on Linux? *Default:* yes,
   with the deviation stated in the security documentation.
8. **C7 Code signing certificate or signing service** (yearly cost). *Default:* none; builds marked
   unsigned and not published.
9. **D2 Priority:** where Windows comes relative to GO-4, the SQL beta and 1.0. *Default:* after the
   beta of both priority workflows; W1 (compile) may start earlier because it only touches code.

## Revision

- v1 (2026-11-18): proposed.
