# Technology evaluation

**Date:** 2026-10-30 · versions and licenses from `cargo info` on that date.

## Criteria

| # | Criterion | Why |
|---|---|---|
| K1 | License permissive (MIT, Apache-2.0, BSD, ISC, Zlib) | project licensing, publication |
| K2 | Proven in an editor or IDE | large documents, latency |
| K3 | Platforms macOS, Linux, Windows | reach |
| K4 | Text rendering quality (shaping, IME, ligatures, bidi) | core of an IDE |
| K5 | Accessibility (screen readers) | usability |
| K6 | API stability, docs, community | maintenance cost |
| K7 | Pure Rust / few native build steps | "Rust only" goal, CI simplicity |

## UI toolkit

| Candidate | Version | License | K2 | K4 | K5 | K6 | Verdict |
|---|---|---|---|---|---|---|---|
| **gpui** | 0.2.2 | Apache-2.0 | ✅ Zed editor | ✅ GPU text, IME | ⚠️ limited | ⚠️ pre-1.0, Zed-driven, sparse docs | **chosen** (ADR-0003) |
| egui | 0.37.0 | MIT OR Apache-2.0 | ⚠️ immediate mode; fine for tools, heavier for huge buffers | ✅ | ✅ AccessKit | ✅ stable cadence | **fallback** |
| iced | 0.14.0 | MIT | ⚠️ some editors | ✅ | ⚠️ | ⚠️ breaking releases | not chosen |
| floem | 0.2.0 | MIT | ✅ Lapce | ✅ | ⚠️ | ⚠️ small community | not chosen |
| slint | 1.18.1 | GPL-3.0 OR proprietary royalty-free | ⚠️ | ✅ | ✅ | ✅ | **excluded by K1** |

Note: `gpui` lives in the Zed repository; the Zed *application* is GPL. Only
crates that are themselves Apache-2.0 may be used.

## Other components

| Area | Choice | Version | License | Alternatives considered |
|---|---|---|---|---|
| Text storage | `ropey` 1.x | 1.6.1 | MIT | own piece table (later, if profiling demands); `ropey` 2.0 is beta |
| Parsing / highlighting | `tree-sitter` + grammars | 0.27.1 | MIT (grammars: check each) | `syntect` (regex grammars, slower incremental) |
| Language intelligence | external servers over LSP; `lsp-types` | 0.97.0 | MIT | in-process analysers (not scalable across 14 languages) |
| Rust support | `rust-analyzer` as external LSP server | — | MIT/Apache | — |
| Python / TS / Java / SQL | pyright or basedpyright, typescript-language-server, Eclipse JDT LS, own SQL service + sqls | — | per server; launched as separate processes, not distributed | — |
| Debugging | DAP; own `serde` types generated from the published DAP JSON schema | — | schema MIT | `dap-types` 0.0.1 (too immature) |
| Database | native driver per database behind one session interface (ADR-0011): `tokio-postgres` first | 0.7.18 | MIT OR Apache-2.0 | `mysql_async` 0.37.1, `rusqlite` 0.41.1 next; `sqlx` 0.9.0 dropped (text values of unknown types, error positions) |
| Git | `gix` + `git` CLI fallback | 0.90.0 | MIT OR Apache-2.0 | `git2` (C library libgit2) |
| Terminal | `alacritty_terminal` + `portable-pty` | 0.26.0 / 0.9.0 | Apache-2.0 / MIT | `vte` low-level |
| Plugins | WebAssembly component model via `wasmtime` | 50.0.1 | Apache-2.0 WITH LLVM-exception | dynamic libraries (unsafe ABI), scripting runtimes |

## What was built instead (2026-11-03)

The table above records the evaluation. Where the implementation chose differently, the code and
[ARCHITECTURE.md](../../ARCHITECTURE.md) are authoritative:

- **Language intelligence:** own LSP types and JSON-RPC over stdio; `lsp-types` is not used.
- **Terminal:** `portable-pty` with the `vte` parser and an own screen model; `alacritty_terminal`
  is not used.
- **Plugins:** core WebAssembly modules on the `wasmi` interpreter without host functions
  ([ADR-0016](../adr/0016-plugins-as-wasm.md)); `wasmtime` and the component model are not used.
- **Database:** besides the three drivers named above, `oracle` (Instant Client) and `tiberius`
  (SQL Server); MariaDB uses `mysql_async`.
- **Git:** `gix` in-process; the `git` program for fetch/pull/push, commit hooks and conflict
  resolution.

## Not done yet

- License check of individual tree-sitter grammars and language servers — M4.

The GPUI performance spike (ADR-0003 acceptance criteria) passed on 2026-11-03
([SPIKE_GPUI.md](SPIKE_GPUI.md)).
