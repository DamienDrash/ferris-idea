# SPEC-0210: Plugin extension points

- **Version:** 2 · **Date:** 2026-11-15 (v2: widgets follow a file opened or a tab chosen, behaviour 4a; the
  widgets and the tool window's sections and rows in the semantic tree, behaviour 8; AK-5)
- **Sources:** GEN (what IDE plugins most often add: completion contributors, support for file types
  the IDE does not know — highlighting, comments —, status bar widgets and tool windows; run
  configuration types for further tools; F-080), STD (WebAssembly core specification), SPEC-0104,
  SPEC-0156, SPEC-0166, SPEC-0072 (completion), SPEC-0167 (tool windows), ADR-0016 (amendment
  2026-11-03, extension points), ADR-0008
- **Feature matrix rows:** F-080

## Purpose
Plugins can add more than commands: suggestions in the completion list, highlighting and comments
for file types Ferris IDEA does not know, a widget in the status bar and a list in a tool window —
all inside the sandbox of ADR-0016, with no new host functions.

## Choice of extension points
Language support plugins (a file type with highlighting and comments, then completion) are the
largest group of plugins people install, followed by plugins that show information about the
current file or project (status bar widgets, tool windows). These four come first. Run
configuration types are left out: they start programs, which the sandbox deliberately cannot do
(ADR-0016); they need a decision on a reviewed "run a command the user confirmed" capability first
(open question in the status file). [GEN]

## Behaviour
1. **Completion (`complete`):** an action of kind `complete` (optional `files`, as for `inspect`)
   gets `{path, language, text, offset, prefix}` (`offset`: caret in characters; `prefix`: the
   identifier characters before it) and answers `{items: [{label, insert, detail}]}` (`insert`
   defaults to `label`). It runs with a fifth of an action's instruction budget, the first 200 items
   count, and is not offered as a command. [GEN, ADR-0016]
2. **In the editor:** when the completion list opens (⌃Space or while typing) the matching plugins'
   items are added after the language's own, with the plugin's name in the detail column and the
   same prefix filter; in a file without a language server (plain text, a plugin file type) the
   list shows the plugins' items alone. A failing plugin adds nothing and is named in the status
   bar after an explicit ⌃Space. [GEN]
3. **File types (manifest `file_types`):** a plugin declares `[{name, extensions, line_comment,
   block_comment: [open, close], keywords, case_sensitive}]` (`case_sensitive` defaults to true;
   at most 2 000 keywords; extensions without the dot). No plugin code runs for them: Ferris IDEA
   colours comments, strings in `"` or `'` (with `\` escapes), numbers and keywords of such files
   itself, using the colours of its own languages. A file type never replaces a built-in language
   (an extension Ferris IDEA highlights stays Ferris IDEA's); the first enabled plugin by file name wins
   between plugins. Comment with Line Comment (⌘/) uses `line_comment`. [GEN]
4. **Status bar widget (`status`):** an action of kind `status` (optional `files`) gets the editor
   context of SPEC-0156 behaviour 1 and answers `{text, tooltip}`; `text` is cut to 40 characters.
   It runs 300 ms after the caret or the text of the active editor last changed, with a fifth of the
   budget, and its text stands in the status bar before the caret position; a click shows the
   tooltip as the status message. A failing plugin shows `⚠ <plugin>`, its click the reason.
   Nothing is shown without an editor. [GEN]
   4a. **(v2)** The widgets also run when the active editor changed without an edit or a caret move —
   a file opened, another tab chosen —, so the status bar never shows nothing or the widgets of the
   file before until the caret moves (found by the window test of AK-5). [GEN]
5. **Tool window (`panel`):** an action of kind `panel` gets `{project, active}` (the project's
   folder name and the active file relative to it, if any) and answers `{rows: [{text, path, line}]}`
   (at most 1 000 rows). The tool window **Plugins** (right side, Search Everywhere "Plugins Tool
   Window") shows one section per panel action with its title and rows; it runs them when it opens
   and on ↻ Refresh. Clicking a row with a `path` opens that project file at `line` (0-based); a
   path outside the project is not opened. Project access is as granted for `project-read`
   (SPEC-0156). [GEN]
6. **Sandbox:** every extension point runs in a fresh instance with the limits of SPEC-0104; none
   gets a new host function, and none writes files. [ADR-0016]
7. **MCP:** `plugins` lists each plugin's `extensions` (completion, status bar, tool window,
   inspections, file types with their extensions); `run_plugin_action` also runs `complete`
   (`path`, `offset`), `status` (`path`, `selection`) and `panel` actions and returns their checked
   answers; `plugin_file_type` names the plugin file type of a workspace file and returns its
   highlighted spans. [ADR-0008]
8. **(v2) Semantic tree (SPEC-0319):** each status widget is a button named with its text
   (`plugin-status-<n>`); each tool window section is a heading named with its title
   (`plugin-panel-<n>`) and each row a list item named with its text (`plugin-panel-row-<n>-<i>`,
   Click, no effect outside the window). [ADR-0027]

## Acceptance criteria
- **AK-1** Runtime with the sample plugins under `tests/plugins/` (one per extension point, kept as
  WebAssembly text and built module): completion items checked and limited, status text cut, panel
  rows, a file type's highlighting (comment, string, number, keyword, case-insensitive) and its
  line comment, built-in extensions refused (`crates/plugins/tests/samples.rs`).
- **AK-2** MCP: extensions listed, `complete`/`status`/`panel` run, `plugin_file_type`
  (`crates/mcp/tests/it/protocol.rs`: `plugin_extension_points`).
- **AK-3** Window self-test (`plugin_extension_steps`): a plugin file type colours a file and ⌘/
  comments a line with its prefix; ⌃Space lists the plugin's completion and Enter inserts it; the
  status bar shows the widget's text; the Plugins tool window lists the panel's rows and a click
  opens the file.
- **AK-4** Hand test (round 4, no. 150): install the four sample plugins from `tests/plugins/` and
  use each extension point.
- **AK-5 (v2)** Window test with keys and clicks by the semantic tree
  (`crates/workbench/tests/it/f080_window.rs`: `hand_test_150_the_four_extension_points`): with the
  four sample plugins, `notes.conf` opened shows "Lines: 3" without a caret move (behaviour 4a), is
  coloured by the plugin file type, ⌘/ puts `#` and ⌘Z takes it back, ⌃Space lists
  `plugin_hello` and `plugin_world` and Enter inserts the first, Find Action opens the Plugins
  tool window with "Sample Panel: Bookmarks", and a click on a row opens `notes.conf` at its line.

## Out of scope
Run configuration types (see above), plugin settings pages, own UI drawn by plugins, events on
save, plugin-defined colours, Tree-sitter grammars from plugins, a marketplace.

## Known deviations from the reference
The reference's plugins register these extension points with JVM code that runs inside the IDE and
can draw any UI; Ferris IDEA's plugins answer with data that Ferris IDEA checks and shows itself
(IMP-0037). File types are declared, not programmed, so a broken plugin cannot slow typing.
