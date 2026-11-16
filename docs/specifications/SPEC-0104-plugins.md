# SPEC-0104: Plugins

- **Version:** 2 · **Date:** 2026-11-15 (v2: AK-5)
- **Sources:** GEN (IDEs can be extended with plugins that add actions; F-080), STD (WebAssembly core
  specification: modules, linear memory, exports), ADR-0016
- **Feature matrix rows:** F-080

## Purpose
Let people and teams add their own text actions to IDECraft without trusting them with their machine.

## Behaviour
1. **Module:** a plugin is a WebAssembly module (at most 16 MiB) that imports nothing and exports
   `memory`, `idecraft_alloc(len: i32) -> i32` (memory for the host to write into),
   `idecraft_manifest() -> i64` and `idecraft_run(action_ptr, action_len, input_ptr, input_len: i32)
   -> i64`. A returned `i64` is a pointer (high 32 bits) and a length (low 32 bits) of UTF-8 text; a
   negative value is an error code. The manifest is JSON: `name`, `version`, `description`, `actions`
   with `id`, `title` and `kind` (`replace`, the default, or `message`). [STD, ADR-0016]
2. **Installing:** plugins are the `*.wasm` files in the user's IDECraft settings folder `plugins/`
   (or `IDECRAFT_PLUGINS`), read when Search Everywhere opens; a workspace cannot add any. A file that
   is not a valid plugin is skipped and named in the status bar. [GEN, IMP-0028]
3. **Running:** each action appears in Search Everywhere as "Plugin name: Title" (id
   `Plugin.<file>.<action>`). It runs off the UI thread in a fresh instance on the selection of the
   active editor, or its whole text without a selection; `replace` actions replace that text (one undo
   step), `message` actions show their output in the status bar. [GEN]
4. **Sandbox:** no host functions (a module with imports does not load), at most 64 MiB of memory,
   50 million instructions per call ("stopped: it ran too long"), 4 MiB of input or output. [ADR-0016]
5. Agents list plugins with `plugins` and run an action on a text with `run_plugin_action`; nothing is
   written. [ADR-0008]

## Acceptance criteria
- **AK-1** A plugin in the WebAssembly text format: manifest, a replacing action on UTF-8 text, a
  message action, an unknown action; an endless action stopped by the budget; a module with an import
  refused; a folder with a broken file (`crates/plugins/tests/runtime.rs`).
- **AK-2** MCP: an empty folder, an installed plugin listed, its action run, found by
  `search_everywhere` (`crates/mcp/tests/it/protocol.rs`: `plugin_tools`).
- **AK-3** In a real window (`idecraft ui --selftest`): an installed plugin's action replaces the
  selected word with capitals.
- **AK-4** By hand: a plugin built from Rust for `wasm32-unknown-unknown` installed and run.
- **AK-5 (v2)** Window test with keys (`crates/workbench/tests/it/f080_window.rs`:
  `hand_test_27_a_text_plugin_from_find_action_next_to_a_broken_one`): next to a file that is no
  module, the text plugin is loaded alone, ⇧⌘A "Upper Case" and Enter replace the selection, typing
  goes on, and Settings › Plugins lists the broken file without a manifest.

## Out of scope
Host functions (project files, editor state, network), inspections, completion or UI from plugins, a
plugin manager and marketplace, signing, updates, per-workspace plugins, the reference platform's
plugins.

## Known deviations from the reference
The reference's plugins are JVM code with full access to the IDE and the machine; IDECraft's plugins
are sandboxed text actions (improvement IMP-0037). Not compared side by side.
