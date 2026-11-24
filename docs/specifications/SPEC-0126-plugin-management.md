# SPEC-0126: Managing plugins in Settings

- **Version:** 2 · **Date:** 2026-11-05
- **Sources:** GEN (IDEs list installed plugins in Settings › Plugins, install one from a file on
  disk, switch plugins off without removing them, and uninstall them; F-080), SPEC-0104 (plugins),
  SPEC-0105 (Settings dialog), ADR-0008, ADR-0016
- **Feature matrix rows:** F-080, F-002

## Purpose
People can see, add, switch off and remove their plugins in the window instead of copying files
into a settings folder.

## Behaviour
1. **Page:** Settings has a **Plugins** page (found by "plugin", "extension", "install", "wasm").
   It lists every plugin file of the user's plugins folder (SPEC-0104 behaviour 2) by file name:
   name and version, description (else the file name), number of actions, "disabled"; a file that
   does not load shows ⚠ with the reason. The folder is shown at the bottom. [GEN]
2. **Install Plugin from Disk…** (button, ⌘O / Ctrl+O on the page) asks for a `.wasm` file. It is
   loaded and checked like an installed plugin first; only a valid plugin is copied into the folder
   (through a temporary file, so a failed copy leaves nothing behind) and replaces an installed
   plugin of the same file name, enabled or not. The note says what was installed or why not. [GEN]
3. **Enable / disable:** Enter or a double click switches the selected plugin off or on. A disabled
   plugin stays in the folder as `<name>.wasm.disabled` and is not loaded; its actions leave Search
   Everywhere at once. [GEN]
4. **Remove:** Delete asks ("Press Delete again to remove …"), a second Delete removes the file; any
   other key cancels. On macOS Delete is the ⌫ key: it removes while the search field is empty, and
   fn+⌫ removes while it has text (v2, hand test 53, D18: only fn+⌫ removed). [GEN]
5. **MCP:** `plugins` also lists disabled plugins (`disabled`: file, name, problem). Installing,
   enabling and removing stay with the user, as plugins are trusted by the person, not by an agent
   or a workspace (IMP-0037). [ADR-0008]

## Acceptance criteria
- **AK-1** Install refuses a broken module and a file without `.wasm`; install, disable (not loaded),
  re-install over a disabled copy, enable, broken file listed with its problem, remove
  (`crates/plugins/tests/runtime.rs`: `managing_installed_plugins`); the page is found by its
  keywords (`settings_model::tests`).
- **AK-2** MCP: a disabled plugin is listed apart and offers no actions (`crates/mcp/tests/it/protocol.rs`:
  `plugin_tools`).
- **AK-3** Window self-test (`plugin_steps`): the page lists the plugin; Enter disables and enables it
  (its action goes and comes back); installing from disk adds a second; ⌫ twice removes it (v2).
- **AK-4** Hand test (round 4): install the example plugin from Downloads, switch it off and on,
  remove it.

## Out of scope
A marketplace, updates, signatures, plugin settings, restart prompts (none are needed: plugins load
on use), dependencies between plugins.

## Known deviations from the reference
The reference installs JVM plugins that usually need a restart; Ferris IDEA's plugins are sandboxed
modules that take effect at once. Not compared side by side.
