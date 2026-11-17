# SPEC-0004: Language identification

- **Version:** 9 · **Date:** 2026-11-16 (v9: F-030 check without a screen — an Emacs or Vim
  modeline names the language, behaviour 18; a hashbang line or modeline typed and saved takes
  effect at once, behaviour 19; the status bar names the file type and opens Override File Type,
  behaviour 20; IMP-0495, IMP-0496; v8: every other typed filter and the merge result take
  committed text, behaviour 17b; v7: the speed search of the Database tool window and
  the filter of the Spring tool window take committed text too, behaviour 17a; v6: OBS-0052 G1–G4 — icons follow the detected language
  and an overridden file is marked, Override and Revert are offered only where they apply, the list
  of file types takes committed text and shows icons, the exact match is selected; v2: JSON `.json`/`.jsonc` for SPEC-0027; v3: line comment token for SPEC-0056;
  v4: XML, YAML, Markdown, TOML, Shell, Dockerfile, Groovy, Scala, SCSS, Less, Swift, Lua, R; v5: the
  language from the hashbang line, file name patterns and per-file overrides of the project, the
  actions Override File Type, Revert File Type Override and Associate with File Type, MCP `file_type`)
- **Sources:** GEN (conventional file extensions; a script's first line `#!` names its interpreter),
  STD (LSP `languageId` values), DOC v5 (an IDE lets the user associate file name patterns with a
  file type and override the type of a single file), OBS v5 (OBS-0031 item 5: the reference's
  editor tab menu offers Override File Type), OSS v5 (`JetBrains/intellij-community`, action ids
  `OverrideFileTypeAction`, `ReverteOverrideFileTypeAction` and `AssociateWithFileType`, the first
  two grouped in the Project view's menu and the editor tab menu, the third in both menus after
  New; ids only, no code), OBS v6 (OBS-0052: the reference's tree shows a hashbang script and an
  overridden file with their language's icon and the override with a small mark; Find Action
  offers Revert only for an overridden file and Override only for one without; its list of file
  types has icons, narrows as one types and selects the exact match), GEN v9 (the modeline forms
  of Vim — `vi:`, `vim:`, `ex:` with `ft=`, `filetype=`, `syntax=` in the first or last five
  lines — and of Emacs — `-*- mode: … -*-` on the first line, the second after `#!`; a status bar
  that names the file type and changes it on a click, as common editors do)
- **Feature matrix rows:** F-030

## Purpose
Decide which language service handles a file.

## Behaviour
1. The language is derived from the file extension, case-insensitively. [GEN]
2. Well-known extensionless names are recognised (`Gemfile`, `Rakefile` → Ruby). [GEN]
3. `.h` maps to C++ because a C++ service also handles C headers. [GEN]
4. Each language reports the identifier the LSP specification lists for it. [STD]
5. Unknown files have no language (plain text). [GEN]
6. JSON (`.json`, `.jsonc`) is recognised for syntax highlighting (SPEC-0027). [GEN]
7. Each language reports its line comment token (`//`, `#`, `--`) or none (HTML, CSS, JSON, XML,
   Markdown) for SPEC-0056. [STD]
8. **More languages (v4):** XML (`.xml`, `.xsd`, `.xsl`, `.xslt`, `.svg`, `.fxml`, `.iml`), YAML (`.yaml`,
   `.yml`), Markdown (`.md`, `.markdown`), TOML (`.toml`, `Cargo.lock`, `Pipfile`), Shell (`.sh`,
   `.bash`, `.zsh`, `.bashrc`, `.bash_profile`, `.zshrc`, `.zprofile`, `.profile`), Dockerfile
   (`Dockerfile`, `Dockerfile.*`, `Containerfile`, `.dockerfile`), Groovy (`.groovy`, `.gradle`, `.gvy`,
   `Jenkinsfile`), Scala (`.scala`, `.sc`, `.sbt`), SCSS, Less, Swift, Lua and R (`.r`, `.R`). The LSP ids
   are those the specification lists (`shellscript` for Shell). `build.gradle.kts` stays Kotlin. [GEN, STD]

9. **Hashbang (v5):** a file whose name gives no language (behaviours 1, 2, 8) and whose first
   line starts with `#!` gets the language of the interpreter it names. The interpreter is the last
   part of the first word's path; after `env`, its options (words starting with `-`) and
   assignments (`NAME=value`) are skipped. Version digits and dots at the end are ignored
   (`python3.12` is `python`). `python`, `pypy` → Python; `sh`, `bash`, `zsh`, `dash`, `ksh`,
   `ash` → Shell; `node`, `nodejs` → JavaScript; `ts-node`, `tsx` → TypeScript; `ruby` → Ruby;
   `php` → PHP; `lua`, `luajit` → Lua; `Rscript` → R; `groovy` → Groovy; `scala` → Scala;
   `swift` → Swift. At most the first 256 bytes of the first line count, only for absolute
   paths of regular files (v9: read with the modeline, behaviour 18); the answer is kept until
   the file's modification time changes. [GEN]
10. **File name patterns (v5):** a project keeps its own associations in
   `.idecraft/file-types.toml`, table `patterns`: a pattern on the file name (`*` any run of
   characters, `?` one character, compared ignoring case) and the LSP id of its language, or
   `text` for plain text. A matching pattern wins over behaviours 1, 2, 8 and 9; a pattern without
   wildcards wins over one with them, then the longer pattern. [DOC, GEN]
11. **Overrides (v5):** the same file's table `overrides` names single files by their path
   relative to the project folder (`/` between parts) and their language or `text`. An override
   wins over everything else. [DOC]
12. **Where it counts (v5):** every place that asks for a file's language — highlighting, comment
   tokens, language servers, formatting, inspections, the Project tree, MCP — follows behaviours
   9–11 for the files of a project whose window or MCP server is open. The file is read again when
   it changed on disk, at most once a second. An open editor of a file whose language changed
   gets the new highlighting and comment tokens at once; its text, caret, undo steps and unsaved
   changes stay. [GEN]
13. **Actions (v5):** they act on the Project tree's selected files while the tree has the focus,
   else on the active editor's file; folders are left out; a remote project says that file types
   are kept on the local machine only and changes nothing.
   - `OverrideFileTypeAction` "Override File Type": a list with Plain Text and every language by
     its name; typed text narrows it, ↑/↓ and Enter choose, Escape closes. The choice is saved as
     the override of every target file; the status bar says `app.cfg is treated as YAML` (several
     files: `3 files are treated as YAML`).
   - `ReverteOverrideFileTypeAction` "Revert File Type Override" (the reference's id, spelt as
     it is): removes the overrides of the target files; the status bar says `app.cfg is detected
     from its name again`, or `app.cfg has no file type override`.
   - `AssociateWithFileType` "Associate with File Type…": for a file whose language is not known
     the same list without Plain Text; the choice is saved as the pattern `*.ext` of the file's
     extension (the whole name when it has none) and the status bar says `*.tpl files are
     treated as HTML`. For a file with a language it says `notes.md already is Markdown` and
     opens nothing.
   - The editor tab menu and the Project tree's menu of a file hold "Associate with File Type…"
     (enabled when the file's language is not known), "Override File Type" and "Revert File Type
     Override" (enabled when the file has an override), after Rename File… in the tab menu and
     after New in the tree's. Find Action finds the three; no bundled keymap binds them; UI
     automation declares them as writes. [OSS, OBS, GEN]
14. **MCP (v5):** `file_type` with `action`: `get` (default; `path` → `language` as LSP id or
   null, its `title`, `source` one of `override`, `pattern`, `name`, `hashbang`, `none`, and the
   `pattern` that matched), `list` (the patterns and overrides of the project), `override` (`path`,
   `language`: an LSP id or `text`), `revert` (`path`), `associate` (`pattern`, `language`),
   `dissociate` (`pattern`). The four changing actions report what would change with `dry_run`
   (default true); `dry_run: false` needs the `write` right and is logged in
   `.idecraft/agent-log.jsonl`. An unknown language is refused with the list of ids; a path outside
   the workspace is refused. `detect_language` follows behaviours 9–11 for a workspace path.
   [ADR-0008]

15. **Icons (v6):** the Project tree and the editor tabs (and the lists that take a tab's icon,
   such as Recent Files and the tab list) show a file's icon by the language behaviours 9–11
   decided: a file whose language comes from an override, a pattern or its hashbang line shows
   that language's icon, plain text the text icon; a file decided by its name keeps the icon of
   its name (`.gitignore`, images and archives have icons of their own). A language without an
   icon of its own shows the text icon, never the unknown one. A file with an override carries a
   small mark of IDECraft's own drawing on the lower right of its icon. The decision is kept per
   file for at most a second and forgotten at once after one of the actions of behaviour 13. Java
   and Kotlin files whose language stays Java or Kotlin keep the icon of their primary
   declaration (SPEC-0131 v3 behaviour 7). [OBS, GEN]
16. **Offered where it applies (v6):** Find Action offers "Override File Type" only while one of
   the target files (behaviour 13) has no override, and "Revert File Type Override" only while one
   has an override; with no target file neither is offered. The tab menu and the Project tree's
   menu of a file hold "Override File Type" for a file without an override and "Revert File Type
   Override" for one with it, never both; "Associate with File Type…" stays as in behaviour 13.
   [OBS]
17. **The list of file types (v6):** the list of behaviour 13 takes the text the platform's text
   input commits, as every input field does, not the key behind it: synthetic input with a Unicode
   string, dead keys and input methods type what they mean, upper case and letters such as `ä`
   included. Keys that act on the list (↑/↓, Enter, Escape, ⌫, Space in a list with check boxes)
   keep their meaning. This holds for every list popup built the same way (the Run/Debug and
   Generate lists, the lists at the caret, the run configuration chooser, where a digit still picks
   its row while nothing is typed). Every row shows its icon (behaviour 15; Plain Text the text
   icon). While typed text narrows the rows, the row whose name equals the text, ignoring case, is
   selected; else the first starting with it; else the first shown. [OBS, GEN]
17a. **v7 – Tool window speed searches:** the speed search of the Database tool window
   (SPEC-0089 v5 behaviour 10: a typed letter opens it, letters extend it) and the typed filter of
   the Spring tool window (SPEC-0115 v5 behaviour 15) take committed text in the same way; their
   keys (↑/↓, Enter, Escape, ⌫, shortcuts with ⌘/⌃/⌥) keep their meaning. The Project tree has no
   speed search in IDECraft, so nothing changes there. [GEN]
17b. **v8 – Every typed filter:** the rule of behaviour 17 holds wherever IDECraft builds text from
   typing outside an editor or input field: the Git log's commit filter, the search of its menus,
   the branches pane's filter, the name of a new branch and the name field of the commit menu
   (SPEC-0086); the Structure popup and tool window (SPEC-0079); the intentions list of ⌥↩
   (SPEC-0076); Recent Files, the tab list and the Switcher (SPEC-0059); the dependency analyzer's
   filter of the build tools window (SPEC-0100); the process list of the profiler (SPEC-0120); and
   the result column of the merge window (SPEC-0101). Space keeps marking a branch in the branches
   pane; shortcuts with ⌘/⌃ (and ⌥ where they had a meaning) keep it. The mnemonic keys of the
   Bookmarks popup and the answer keys of the merge window's questions pick by key, not by text,
   and stay as they are. [GEN]
18. **v9 – Editor modelines:** a file whose name gives no language (behaviours 1, 2, 8) and which
   no override or pattern decides takes the language its editor modeline names, before its
   hashbang line (behaviour 9): Emacs' `-*- mode: python -*-` or `-*- python -*-` on the first
   line, or on the second when the first is a hashbang line; Vim's `vim:`, `vi:` or `ex:` at the
   start of a line or after a blank, with the option `ft=`, `filetype=`, `syntax=` or `syn=`
   (`vim: set ft=yaml :`, `vim:ft=rust`), on one of the first or the last five lines. Modes are
   compared ignoring case and an Emacs `-mode` suffix; besides each language's LSP id and name,
   the common short names count (`py`, `sh`, `bash`, `zsh`, `shell-script`, `js`, `js2`, `ts`,
   `c++`, `nxml`, `md`, `gfm`, `conf-toml`, `less-css`, `ess-r`, `kt`, `rs`, `yml`). A mode that is
   no language of IDECraft (`perl`) decides nothing. At most the first and the last 1 KiB of a
   regular file are read, a start holding a NUL byte counts as no text, and the answer is kept
   until the file's modification time changes. A name that gives a language is never overruled
   by a modeline (the name costs no read). `file_type` reports the source `modeline`; the icon
   follows as for a hashbang line (behaviour 15). [GEN]
19. **v9 – Saved first lines take effect:** after an editor saves its file (⌘S, Save All, an
   agent's edit with save) its language is decided again (behaviours 9–12, 18), so a hashbang
   line or modeline typed and saved gives the new highlighting and comment tokens at once; the
   text, the caret and the undo steps stay. [GEN]
20. **v9 – The status bar names the file type:** while an editor of a local project is active,
   the status bar shows the name of its language after the indent widget (`Plain Text` without
   one; a `.tsx` file `TypeScript`), following every change of behaviours 12, 13 and 19 at once.
   A click on it opens the list of Override File Type (behaviour 13) for the editor's file; the
   tooltip is "File Type". A remote project shows no such widget. [GEN]

## Acceptance criteria
- **AK-1** Priority languages (Python, SQL, JS, TS, Java) are detected from their extensions.
- **AK-2** Special names and unknown files behave as above.
- **AK-3** LSP identifiers are the lowercase ids from the LSP specification.
- **AK-4** The languages of behaviour 8 are detected from their extensions and names; every language
  has its own LSP id (unit test `added_languages`).
- **AK-5 (v5)** Unit tests `file_types::tests`: the interpreters of behaviour 9 with `env`,
  options, assignments and version digits, a file whose name already says its language; the
  order of behaviours 10 and 11 (override over pattern over name over hashbang, exact over
  wildcard over shorter wildcard), `text`, reading and writing the file, a change on disk seen
  after a second, a file outside every project untouched.
- **AK-6 (v5)** Window test of the test harness `crates/workbench/tests/it/file_types.rs` with
  real keys: an extensionless script with `#!/usr/bin/env python3` is Python in the editor; Find
  Action › Override File Type on `app.cfg`, typed `yaml`, Enter: the editor comments a line with
  `#` (⌘/), the unsaved text and the undo step stay, the override is in the file; Revert brings
  plain text back; Associate with File Type on `page.tpl` with `html` saves `*.tpl`; with the
  tree focused the override acts on two selected files; the tab and tree menus hold the three
  items with their enabled state.
- **AK-7 (v5)** MCP (`crates/mcp/tests/it/protocol.rs`: `file_type_tool`): get with each source,
  dry run and write of override, revert, associate, dissociate, the `write` right, an unknown
  language and a path outside the workspace refused, `detect_language` following an override.
- **AK-8 (v5)** The three actions: titles in Find Action and no shortcut
  (`everywhere::tests::file_type_actions_have_titles`), the menus
  (`context_menu::tests::file_type_items_in_the_tab_and_tree_menus`), writes for UI automation
  (`security::tests::file_type_actions_are_declared_as_writes`).

- **AK-9 (v6)** Icons: unit tests `file_type_icons::tests` (a language's icon for every language,
  never the unknown icon; the icon by source: override, pattern and hashbang give the language's
  icon, a name keeps its own); window test `icons_follow_the_detected_language` in
  `crates/workbench/tests/it/file_types.rs`: a hashbang script shows the Python icon in the tree
  and on its tab, an overridden `page.tpl` the HTML icon with the override mark, after Revert the
  icon of its name without the mark.
- **AK-10 (v6)** Offered where it applies: window test `find_action_offers_override_or_revert`
  (Find Action lists Override and not Revert for a file without an override, Revert and not
  Override after the override); `context_menu::tests::file_type_items_in_the_tab_and_tree_menus`
  (one of the two per state).
- **AK-11 (v6)** Committed text: window test `typed_text_reaches_the_file_type_list` — key events
  whose key is `a` but whose committed text is `H`, `T`, `M`, `L` give the filter `HTML` with the
  row HTML selected, the key event itself is left to the text input; `simulate_input` with upper
  case and `ü`, a German layout's key committing `ä`; ↑/↓, ⌫ and Escape keep their meaning; the
  rows carry icons; window test `committed_text_filters_the_chooser` in
  `crates/workbench/tests/it/run_debug_actions_keymap.rs`; unit tests
  `committed_text::tests::committed_text_goes_into_the_filter` and
  `choice_view::tests::the_exact_match_is_selected`.

- **AK-12 (v7)** Window tests `committed_text_opens_and_extends_the_database_search` and
  `committed_text_filters_the_spring_window` in `crates/workbench/tests/it/speed_search_text.rs`:
  key events of the key `a` committing other letters (and `ä`) give the typed text; real keys with
  Shift go on with it; ⌫ and Escape keep their meaning.

- **AK-13 (v8)** Window tests in `crates/workbench/tests/it/filters_text.rs` (Git log commit
  filter and branches pane, Structure, intentions, tab list, profiler process list, merge result)
  and the unit test `build_tools_view::tests::committed_text_narrows_the_analyzer`: key events of
  the key `a` committing other text (also `ü`/`ä`) give that text, Shift with real keys, ⌫ takes
  the last character, Escape keeps its meaning.

- **AK-14 (v9)** Unit tests `file_types::tests::{modelines_name_the_language,
  files_with_a_modeline}`: the Emacs and Vim forms of behaviour 18 with their places (first line,
  second after `#!`, first or last five lines, not the middle), unknown modes and lines that only
  look alike decide nothing, a modeline before the hashbang line, a file longer than 1 KiB read at
  its end, a name that says its language not overruled, a file with NUL bytes not read as text;
  MCP `file_type_tool` reports the source `modeline`.
- **AK-15 (v9)** Unit test `workbench_file_types::tests::the_status_bar_names_the_file_type`
  (a language's name, Plain Text, the React ids of `.tsx`/`.jsx`).
- **AK-16 (v9)** Window test `modelines_saved_hashbangs_and_the_status_bar` in
  `crates/workbench/tests/it/file_types.rs`: `settings.conf` with `# vim: set ft=yaml :` opens as
  YAML and the status bar says `YAML`; an extensionless file is Plain Text, a hashbang line typed
  with real keys changes nothing before ⌘S and makes it Python after, text kept and not modified;
  a real click on the status bar widget opens the list, `plain text` and Enter save the override
  and the widget says `Plain Text`.

## Out of scope (v5)
A Settings page that lists and edits the patterns (they are edited through Associate with File
Type, the file and MCP); patterns of the whole IDE; ignored files and folders; file types of
one's own with keywords; detection from the content beyond the hashbang line.

## Known deviations from the reference (v5)
The reference keeps file name patterns for the whole IDE in its settings; IDECraft keeps them,
like the overrides, in the project (`.idecraft/file-types.toml`), so they travel with the project
and agents see the same as the window. (v6) The override mark is IDECraft's own drawing, not the
reference's arrow. Compared side by side in OBS-0052 (G1–G4 closed in v6). (v9) Editor modelines
(behaviour 18) and the file type in the status bar (behaviour 20) go beyond what the reference
documents (IMP-0495, IMP-0496); neither was compared side by side yet.

## Not covered yet (v9, F-030 check without a screen)
`Makefile`, `CMakeLists.txt` and `.gitignore` have no language: each needs a language of its own
with a grammar for highlighting (a new dependency, its licence checked first). Binary files are
not opened (SPEC-0336 behaviour 3 says why in the status bar); an image viewer and a hex view are
features of their own. Files larger than 20 MiB open whole and stay editable (SPEC-0260 budgets
for 50 MB files) instead of the reference's read-only view in parts.
