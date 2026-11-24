# SPEC-0123: ESLint and Prettier

- **Version:** 6 · **Date:** 2026-11-13 (v6: ESLint's notes on files it does not check are no findings, and an ESLint finding does not hide the result of an action just done, behaviour 2a, AK-14, OBS-0028 T6); 2026-11-13 (v5: the MCP tool `format_file` chooses the formatter as
  Reformat Code does, behaviour 5a, OBS-0028 T3); 2026-11-12 (v4: Optimize Imports in JavaScript,
  TypeScript, Vue and Svelte files through the language server, behaviour 6; v3: carets and
  selections stay on their text after Fix ESLint Problems and Prettier, behaviours 3 and 4, as in
  SPEC-0329 v3); 2026-11-08 (v2: ⌥↩
  fixes of single findings, Reformat Code uses the project's Prettier; OBS-0028 items 7 and 8)
- **Sources:** GEN (IDEs show the project's ESLint findings in the editor, fix them with
  "Fix ESLint Problems" and format with "Reformat with Prettier", using the tools from the
  project's `node_modules`; F-036), DOC (Prettier action id `ReformatWithPrettierAction` with
  ⌥⇧⌘P / Ctrl+Alt+Shift+P; ESLint action `Javascript.Linters.EsLint.Fix`), STD (ESLint CLI:
  `--stdin`, `--stdin-filename`, `--format json`, `--fix-dry-run`, exit codes 0/1/2, 1-based lines
  and UTF-16 columns; flat `eslint.config.*` and legacy `.eslintrc*` configurations; Prettier CLI
  `--stdin-filepath`), ADR-0008; v4: headless finding of 2026-11-12 (Optimize Imports refused a
  TypeScript file whose two imports the language server reports as unused), STD (LSP
  `textDocument/codeAction` with `context.only`, `codeAction/resolve`, the kind
  `source.organizeImports`), OSS (the code-action kinds the language servers announce in
  `codeActionProvider.codeActionKinds` and what they answer, asked from the installed servers:
  typescript-language-server 6.0.2 with TypeScript 5.9.3, svelte-language-server 0.18.6,
  @vue/language-server 2.2.12), SPEC-0122 (Optimize Imports), SPEC-0329 (the same action for
  Python), SPEC-0042 and SPEC-0141 (the language servers)
- **Feature matrix rows:** F-036

## Purpose
JavaScript and TypeScript code is checked and formatted the way the project's own tools and CI
do it, without leaving the editor.

## Behaviour
1. **Tools of the project:** ESLint is used for `.js .jsx .mjs .cjs .ts .tsx .mts .cts .vue` when
   the nearest package folder of the file (up to the workspace root) has
   `node_modules/.bin/eslint` and an ESLint configuration exists there or above it within the
   workspace (`eslint.config.*`, `.eslintrc*`, `eslintConfig` in `package.json`). Prettier is used
   for those and CSS, SCSS, Less, JSON, Markdown, YAML, HTML and GraphQL files when such a folder
   has `node_modules/.bin/prettier`; its configuration is the project's (defaults without one).
   Nothing is downloaded, installed or taken from outside the project; each run has a 30 s time
   limit. [STD]
2. **Findings:** ESLint checks the editor's text — saved or not — on opening, on saving and 0.8 s
   after typing stops (older runs are dropped). Errors and warnings appear as wavy underlines in the
   problem colours, with "ESLint: message (rule)"; a syntax error is a finding. A broken
   configuration is said once in the status bar. [GEN, STD]
   2a. **What is no finding, and what it does not hide (v6, OBS-0028 T6):** ESLint 9 reports a file
   its configuration does not cover, or an ignore pattern matches, as a warning without a rule at
   line 1 ("File ignored because no matching configuration was supplied."). That says ESLint does
   not check the file at all; it is no finding about the code and is not shown, the file has no
   ESLint findings. A fatal message without a rule (a parse error) still is one. Decision for the
   status bar (SPEC-0070 v11 behaviour 4c): ESLint runs after every change, so its finding under
   the caret does not replace the message of the action that made the change — the summary of
   Optimize Imports, Fix ESLint Problems, Reformat Code — for 4 s; afterwards the finding shows as
   before. Before v6, ⌃⌥O in an unconfigured `.ts` file showed "ESLint: File ignored …" at once
   instead of "Removed 2 unused imports". [OBS-0028, GEN]
3. **Fix ESLint Problems** (`Javascript.Linters.EsLint.Fix`, Code menu, Search Everywhere) applies
   what `eslint --fix` changes as one undo step, and the findings are read again. [GEN, DOC]
4. **Reformat with Prettier** (`ReformatWithPrettierAction`, ⌥⇧⌘P on macOS, Ctrl+Alt+Shift+P
   elsewhere, Code menu) formats the editor's text. Only the changed part is replaced (one undo
   step, the caret stays on its text). (v3) For both actions the result is applied as small edits
   where lines differ, and every caret and selection stays on its text as in SPEC-0329 v3
   behaviour 7: moved with the edits in untouched text, before the same visible character in
   reformatted text, where the text was in removed text. Before v3 the whole changed span was one
   edit and a caret inside it kept its offset. Both actions run in the background and apply nothing if the
   text changed meanwhile; without the tool the status bar says what to add to the project. [GEN, DOC]
3a. **(v2) Fixes in ⌥↩:** on a finding with a fix the list offers one entry per fixable finding at the
   caret, named after its rule, and one for the whole file; each applies ESLint's own edits of the last
   check as one undo step. Fixes that overlap are left out of the whole-file entry. Entries appear
   only while the text is the one that was checked. [OBS]
4a. **(v2) Reformat Code:** without a selection, in a trusted project where behaviour 1 finds Prettier
   for the file, Reformat Code does what Reformat with Prettier does. With a selection, without
   Prettier, or in safe mode it formats as before. [OBS]
5. **MCP:** `eslint_file` (findings with line, severity, rule, fixability; with `fix` the fixed
   text, written with `dry_run=false`) and `prettier_file` (`dry_run` default true); writing
   refuses a file that changed meanwhile. [ADR-0008]
5a. **(v5) `format_file` chooses as Reformat Code does:** before v5 the tool always asked the
   language server, so for a file of a project with Prettier it gave another layout than ⌥⌘L in
   the window and than the reference (OBS-0028 "Editorfunktionen neben der Referenz, Teil 1"
   item 3, T3: four spaces and no semicolons against Prettier's two spaces and semicolons).
   Now, called without `range`, in a trusted project where behaviour 1 finds Prettier for the
   file, `format_file` formats the file's text with that Prettier — the same lookup and the
   same trust rule as behaviour 4a. With `range`, without Prettier or in safe mode it asks the
   language server as before (SPEC-0043). The answer has the plan of SPEC-0043 behaviour 3 and
   names what formatted: `formatter` is `prettier` and `formatter_path` its program, or the
   language server's program name and path. With Prettier `tab_size` and `insert_spaces` have
   no effect (its configuration decides), and a failure of Prettier (a syntax error) is the
   answer; the language server is not asked instead. `prettier_file` is unchanged. [OBS-0028 T3, ADR-0008]

6. **(v4) Optimize Imports** (`OptimizeImports`, ⌃⌥O; SPEC-0122 owns the action for Java and
   Kotlin, SPEC-0329 for Python) works in `.ts .tsx .mts .cts .js .jsx .mjs .cjs` files and in
   `.vue` and `.svelte` files (the imports of their script blocks). It stands here because it is
   a JavaScript/TypeScript action done by a tool outside Ferris IDEA, like behaviours 3 and 4, and
   because it has to say how it relates to ESLint and Prettier.
   - **Who does it:** the file's language server (SPEC-0042, SPEC-0141) with its organize-imports
     code actions, on the editor's text — saved or not. Ferris IDEA has no import sorter of its own
     for these languages. ESLint and Prettier of the project are **not** run by this action, even
     when their rules or plugins would order the imports differently; Fix ESLint Problems
     (behaviour 3) and Reformat with Prettier (behaviour 4) stay the way to apply those. [GEN, STD]
   - **Which code actions:** Ferris IDEA reads the kinds the server announces. A server that
     announces both `source.removeUnusedImports` and `source.sortImports` (or kinds below them,
     such as `source.removeUnusedImports.ts`) is asked for the first, and then, on the text that
     results, for the second: remove the unused imports, then sort and combine the rest. Any
     other server that announces `source.organizeImports` (or a kind below it), or announces no
     kinds at all, is asked for `source.organizeImports` once. Reason: typescript-language-server
     6.0.2 answers `source.organizeImports` with sorting and combining only and leaves unused
     imports in place, while the two narrower kinds together do the whole clean-up; the Vue
     language server announces `source.organizeImports` only. Each request names its kind in
     `context.only`, covers the whole text and carries no diagnostics; an action without an edit
     is resolved (`codeAction/resolve`), a command of the action is executed and its edits taken.
     An action that would change another file is refused. [STD, OSS]
   - **Result:** unused imports are gone — whole statements, or single names of a statement that
     keeps others — and the remaining imports stand sorted and combined the way the server does
     it (which may also normalise the spacing inside the braces of a statement it rewrites).
     Imports for their side effects (`import "./x"`) stay. Nothing outside the imports changes.
     [OSS]
   - **In the window:** in the background, as one undo step in which every caret and selection
     stays on its text (SPEC-0329 v3 behaviour 7); nothing is applied when the text changed
     meanwhile. The status bar says in one line what happened: "Removed 2 unused imports and
     sorted the imports (language server)", "Imports sorted (language server)" when nothing was
     removed, "Imports are already optimized" when the text stays the same. The number counts
     imported names — the bound names of the import statements before that are no longer bound
     by an import afterwards — not statements. [GEN]
   - **Without a server:** when no language server for the file is installed, or the server
     offers no organize-imports action, the status bar (MCP: the error) says so, with the install
     hint of SPEC-0042 for a missing server, and nothing is changed. [GEN]
   - **MCP:** `optimize_imports` answers for these files with the fields of SPEC-0329 behaviour 8:
     `changed`, `applied`, `removed` (the names), `left` (always empty here), `by`
     (`language server`), `sorted` (true), `summary` (the status line) and the new `text`;
     `dry_run` defaults to true, writing refuses a file that changed meanwhile. [ADR-0008]

## Acceptance criteria
- **AK-9 (v4, behaviour 6)** Which code actions are asked for which announced kinds; the removed
  names between two texts for default, namespace, named, aliased, type-only and `require`
  imports and for the script blocks of Vue and Svelte files; the status lines (unit tests in
  `js_imports`).
- **AK-10 (v4, behaviour 6)** With the real typescript-language-server: on unsaved text the
  unused imports go, a statement keeps its used names, the rest is sorted and combined, a file
  with nothing to do stays the same, a `.tsx` and a `.js` file work; with the real Svelte and Vue
  language servers the unused import of a script block goes
  (`crates/languages/tests/it/js_imports.rs`; each skipped with a note without its server).
- **AK-11 (v4, behaviour 6)** MCP: `optimize_imports` on a TypeScript file previews with all
  fields and writes with `dry_run=false`; without the server the answer is an error that names
  the missing server and the file stays (`crates/mcp/tests/it/protocol.rs`:
  `typescript_optimize_imports_tool`).
- **AK-12 (v4, behaviour 6)** In the window next to the reference: ⌃⌥O on a TypeScript file with
  unused imports, one undo step, carets on their text (window check open).
- **AK-1** UTF-16 columns, point findings, parse errors; smallest edit; tools only from the project
  and only with a configuration (`js_tools::tests`); the shortcut (`keymap::tests`).
- **AK-2** With ESLint 9 and Prettier 3 installed (`js.sh install` of the reference environment): findings
  with rule and severity, fix, nothing left to fix, syntax error, formatting with the project's
  configuration (`crates/languages/tests/it/js_tools.rs`).
- **AK-3** MCP: findings, fixed text, formatting written (`crates/mcp/tests/it/protocol.rs`:
  `eslint_and_prettier_tools`).
- **AK-4** Window self-test: two errors and a warning appear, Fix ESLint Problems leaves one error,
  ⌥⇧⌘P formats.
- **AK-6 (v2)** Reformat Code on `hello.js` of a project with Prettier gives Prettier's text
  (semicolons, braces); window check next to the reference (OBS-0028).
- **AK-7 (v2)** `js_tools::fixes_at`: UTF-16 fix ranges become byte ranges, one entry per fixable
  finding at the caret, a whole-file entry, nothing away from a finding (unit test).
- **AK-8 (v3, behaviours 3 and 4)** Formatting and fixes keep carets and selections on their text
  (`ferris_idea_editor::reformat` unit tests, shared with SPEC-0329 AK-9).
- **AK-13 (v5, behaviour 5a; OBS-0028 T3)** `format_file` on a JavaScript file of a workspace with
  the project's Prettier: the plan's text is Prettier's (the same text `prettier_file` gives),
  `formatter` is `prettier`, `formatter_path` ends in `node_modules/.bin/prettier`, a dry run
  writes nothing and `dry_run=false` writes the file; in safe mode Prettier is not run
  (`crates/mcp/tests/it/protocol.rs`: `format_file_uses_the_projects_prettier` with a stand-in
  Prettier, `eslint_and_prettier_tools` with Prettier 3, skipped without it); with
  typescript-language-server and without Prettier the answer names the server
  (`format_file_names_the_language_server`, skipped without the server); `ferris-idea call` on the
  file of OBS-0028 T3.
- **AK-14 (v6, behaviour 2a)** ESLint 9.17's note on a `.ts` file outside its configuration and on
  an ignored file give no findings, a fatal parse error still one
  (`js_tools::tests::notes_on_ignored_files_are_no_findings`); the order of the status bar as
  SPEC-0070 AK-11. Window check of ⌃⌥O in `fmt.ts` open.
- **AK-5** Hand test (round 4): a real project (e.g. React with TypeScript) next to the reference.

## Out of scope
Fix or format on save, ESLint suggestions (fixes the user must choose), suppression comments, ESLint and Prettier
in other folders than the project's, Stylelint, TSLint, the Prettier "run for files" pattern.
Optimize Imports (v4): an import order of Ferris IDEA's own or of the project's ESLint/Prettier
rules, adding missing imports, removing other unused code, optimizing on save or for a whole folder.

## Known deviations from the reference
Compared in OBS-0028. Whole-file fixes in ⌥↩ apply one pass only; Fix ESLint Problems repeats like `--fix`.
Optimize Imports (v4) is not yet compared with the reference in the window (AK-12): the order and
the spacing of the imports are the language server's, which need not be the reference's code style.
