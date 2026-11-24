# SPEC-0150: Font family, Code Style and Version Control settings

- **Version:** 6 · **Date:** 2026-11-19 (v6: the project's own code scheme in `.idea/codeStyles`
  between `.editorconfig` and Code Style, behaviour 8, AK-8 to AK-10; gap CS1 of OBS-0109, L2 of OBS-0085,
  decision in the status file of 2026-11-19); 2026-11-18 (v5: the indentation of an opened file is detected from its text where no `.editorconfig` sets it, behaviour 7, AK-7; OBS-0087 W4); 2026-11-15 (v4: the protected branches field in the semantic tree, AK-6; v3: SCSS and Less get rows of their own with two spaces, behaviour 2, OBS-0036 FM1); 2026-11-10 (v2: ligatures checkbox and MCP field)
- **Sources:** GEN (IDE settings choose the editor font, set indentation per language and a right
  margin that the editor draws, and protect branches from force pushes; F-002), OSS (setting
  pages Editor › Font, Editor › Code Style with "Hard wrap at" and per-language "Tabs and
  Indents", Version Control › Git with "Update method", "Allow force push" and "Protected
  branches", `JetBrains/intellij-community`), SPEC-0129, SPEC-0137 (EditorConfig), SPEC-0144,
  ADR-0008
- **Feature matrix rows:** F-002

## Purpose
Make the editor's font and indentation the user's own, and keep shared branches safe from
force pushes.

## Behaviour
1. **Font family:** the Font page's first row "Font" chooses the editor font with Left/Right
   from the bundled JetBrains Mono and the installed monospaced families Ferris IDEA knows (Menlo,
   SF Mono, Fira Code, Source Code Pro, …). Editors, terminal, consoles, diffs and the preview on
   the page use it at once; character positions follow the font's measured width. Size and
   line spacing follow as rows 2 and 3.
1a. **Ligatures** (v2): row 4 "Enable ligatures", a checkbox under the font rows, off by
   default. Space, Enter, Left/Right or a click switch it; editors and the preview on the page
   draw ligatures such as `->` as one glyph at once when the font has them (SPEC-0027 v6). [GEN, OSS]
2. **Code Style** (under Editor): "Hard wrap at" (right margin, 40–400 columns, default 120),
   "Show the right margin", and one row per language (Default, C/C++, CSS, Go, HTML, Java,
   JavaScript, JSON, Kotlin, Less, Markdown, Python, Ruby, Rust, SCSS, SQL, TypeScript, YAML)
   with tabs or spaces, indent and tab width. Left/Right change the indent (the tab width
   follows it), Space or Enter switch tabs and spaces. Built in: tabs for Go, two spaces for
   JSON, YAML, Ruby, SCSS and Less, four spaces otherwise; "Default" applies to languages
   without a style of their own. Tab, automatic indentation and Reformat Code use the file's
   style; an `.editorconfig` property wins over it. Open editors follow at once. [GEN, OSS,
   SPEC-0137] (v3) Until v2 SCSS and Less files followed the CSS row and so were indented with
   four spaces; next to the reference, in a project without settings, a reformatted SCSS and
   Less file has two and a CSS file four (OBS-0036 FM1). The built-in table is the one place
   every default indentation comes from, also what `format_file` asks formatters with
   (SPEC-0043 v5 behaviour 1). A value the user set for "CSS" no longer applies to SCSS and
   Less files. [OBS]
3. **Right margin:** the editor draws a thin line at the right margin column when it is shown.
   [GEN]
4. **Version Control:** "Update method" (Merge or Rebase, the default of Update Project),
   "Allow force push (with lease)" and "Protected branches" (comma-separated, `*` matches any
   text, default `master, main`). The Push dialog refuses ⌥F for a protected target or when force
   push is off, and checks again on Push; MCP `git_push` refuses `force_with_lease` the same way.
   [GEN, OSS]
5. **Search and saving:** the pages are found by their words (indent, tabs, margin, git, force,
   protected, font family); every change is saved at once in the user's settings like the other
   pages (`SPEC-0129`). [SPEC-0129]
6. **MCP:** `ide_settings` returns the new values; `set_ide_settings` takes `font_family`,
   `ligatures` (v2), `right_margin`, `show_right_margin`, `indent` of one language, `update_method`,
   `allow_force_push` and `protected_branches`. [ADR-0008]

7. **v5 – Indentation detected from the file:** when a file is opened (and when its
   `.editorconfig` is read again), the indent unit of Tab, automatic
   indentation and the status bar's indent label comes from the file's own text where no
   `.editorconfig` property (`indent_style`, `indent_size`) sets it; Code Style (behaviour 2)
   gives it only for a file that shows none. Up to the first 1000 lines are read; blank lines
   and lines that continue a block comment (starting with `*` after the indentation) do not
   count. A file whose indented lines mostly start with a tab is indented with a tab. Else the
   step is the indentation, from 2 to 8 spaces, that one line adds over the line before it,
   counted for that line and for each following line that stays at the level it reached; the
   step counted most often wins (on a tie the smaller one), and it needs at least two lines. A
   file with no such step — also one with a single stray indented line — keeps Code Style. A CSS
   file indented with two spaces therefore shows "2 spaces" and Tab inserts two, although the
   CSS row of Code Style says four.
   [GEN: editors detect the indentation in use; OBS-0087 W4: the reference's status bar shows
   "2 spaces" for such a file]

8. **v6 – The project's code scheme:** a project whose `.idea/codeStyles/codeStyleConfig.xml`
   turns on its own scheme (`USE_PER_PROJECT_SETTINGS`, SPEC-0049 v4 behaviour 17) has a third
   level between `.editorconfig` and Code Style. The project of a file is the nearest folder above
   it that holds a `.idea` folder. Without that switch — also when only `Project.xml` exists, or the
   project prefers a named IDE scheme — the scheme does not apply. Order, per value:
   - **Indentation** (indent, tab width, tabs or spaces) of a language: what the file's
     `.editorconfig` sets, else what the scheme's `codeStyleSettings` of that language writes in
     `indentOptions` (`INDENT_SIZE`, `TAB_SIZE`, `USE_TAB_CHARACTER`), else Code Style (behaviour
     2). A value the scheme leaves out comes from Code Style; no default of the reference is filled
     in. `CONTINUATION_INDENT_SIZE` is read but not applied (there is no such setting in Ferris IDEA).
   - **Right margin:** `max_line_length` of the `.editorconfig` (SPEC-0137 v2 behaviour 2a), else
     the scheme's `RIGHT_MARGIN` of the language, else the scheme's own `RIGHT_MARGIN`, else Code
     Style's "Hard wrap at". "Show the right margin" stays Ferris IDEA's switch.
   - **Language ids:** a `codeStyleSettings` element applies to the Code Style row its `language`
     names, compared without regard to case: Java `JAVA`, Kotlin `kotlin`, Python `Python`,
     JavaScript `JavaScript`, TypeScript `TypeScript`, JSON `JSON`, HTML `HTML`, CSS `CSS`, SCSS
     `SCSS`, Less `LESS`, YAML `yaml`, Markdown `Markdown`, SQL `SQL`, Go `go`, Ruby `ruby`, Rust
     `Rust`, C/C++ `ObjectiveC`. Ids of languages without a Code Style row (`XML`, `Groovy`, …) and
     unknown ids are ignored; "Default" has no id. Where two elements name the same row the first
     in the file counts. [OSS for the ids of the open sources; assumption A1 for the others]
   - **Editor:** Tab and automatic indentation follow `.editorconfig`, else the file's own
     indentation (behaviour 7), else the scheme, else Code Style — detection is for editing, as
     in the reference. The editor's right margin line follows the order above.
   - **Reformat Code and MCP `format_file`:** the options passed to the formatter (`tabSize`,
     `insertSpaces`; SPEC-0043 behaviour 1) follow `.editorconfig`, else the scheme, else what the
     editor uses (in the window) or Code Style (MCP); the file's detected indentation does not
     override the scheme. An argument `tab_size` or `insert_spaces` of `format_file` wins over all.
     None of the language servers Ferris IDEA formats with takes a line width among its formatting
     options, so the right margin is not passed to them; formatters with their own configuration
     (Prettier, ruff, black, rustfmt) keep it (SPEC-0123, SPEC-0329).
   - **Which level won:** `format_file` answers `code_style` with `indent_source` and
     `right_margin_source` (`editorconfig`, `project`, `ide`; `argument` for an indentation the
     call gave) and the `right_margin` that applies; `idea_project` answers `code_style.applies`
     (true when the scheme is switched on and its file was read). [ADR-0008; IMP-0510]
   - The scheme files are read again when the file is opened and when its `.editorconfig` is read
     again (behaviour 7); open editors take a changed scheme when a file of `.idea/codeStyles`
     changes, as for a changed `.editorconfig` (SPEC-0137 v2 behaviour 2b).
   **Assumption A1:** the ids of languages whose support is not part of the open sources
   (JavaScript, TypeScript, CSS, SCSS, LESS, SQL, go, ruby, Rust, ObjectiveC) are the ones these
   languages commonly carry in shared `Project.xml` files; only `JAVA` was seen in a file the
   reference wrote (OBS-0109). Comparing without regard to case keeps a different spelling working.

## Acceptance criteria
- **AK-1** Defaults, per-language and "Default" indentation (v3: two spaces for SCSS and Less,
  four for CSS; which row a `.scss`, `.less` and `.css` file follows,
  `fonts::tests`), clamping, protected branch patterns
  (`ide_settings::tests::code_style_and_vcs_defaults`); font choices and language names
  (`fonts::tests::font_choices_start_with_the_bundled_font`).
- **AK-2** MCP: font, margin, a language's indent and protected branches set and saved, an
  unknown language refused; v2: ligatures listed off and switched on (`ide_settings_tools`).
- **AK-3** MCP: force push to `main` refused by the default protection
  (`protected_branches_refuse_force_push`).
- **AK-4** Window self-test: Right on Font changes the family and the editors' font; Left on
  Python's row makes the open Python editor indent with three spaces, Space with a tab; Right on
  Hard wrap moves the margin; Space switches force push off and saves it; in the Push dialog ⌥F
  for `main` is refused with the reason; v2: Space on Enable ligatures switches them on in the
  editor and saves it, Space again switches them off.
- **AK-5** Hand test (round 4, no. 76): font, indentation of Python and Go, right margin and a
  force push to `main` next to the reference.

- **AK-6 (v4)** In a window of the test harness (`crates/workbench/tests/settings_window_checks.rs`:
  `font_shell_and_protected_branches_in_the_window`): the Protected branches field of Version
  Control, found through the semantic tree (a text input with its value, test id
  `settings-protected-branches`), takes `main, release/*` by click and keys, Enter saves it, and the
  field and the file show it.

- **AK-7 (v5, behaviour 7)** `ferris_idea_core::indent_detect::tests`: two-space CSS, four-space
  Java with a javadoc block, tab-indented Go, a file without indentation (none), a tie (the
  smaller step), a single stray indented line (none), one-space continuation lines ignored. In a window of the test harness
  (`crates/workbench/tests/it/window_check_gaps_1118.rs::a_files_own_indentation_wins_over_code_style`):
  a CSS file with two-space indentation shows "2 spaces" and Tab inserts two spaces; with an
  `.editorconfig` saying `indent_size = 4` it shows "4 spaces".

- **AK-8 (v6, behaviour 8)** `ferris_idea_core::project_code_style::tests`: the order per value
  (`.editorconfig` over the scheme over Code Style for indent, tab width, tabs and right margin;
  a value the scheme leaves out comes from Code Style; the language's margin before the scheme's);
  the scheme does not apply without `USE_PER_PROJECT_SETTINGS` or with a preferred IDE scheme;
  the language ids (each Code Style row, case ignored, `XML` and unknown ids ignored, the first of
  two elements counts); the project of a file is the nearest folder with `.idea`.
- **AK-9 (v6, behaviour 8)** MCP (`crates/mcp/tests/it/protocol.rs`:
  `format_file_follows_the_projects_code_scheme`): in a project whose scheme sets Java's indent to
  2 and the right margin to 100, `format_file` on a Java file answers `tabSize` 2 as
  `indent_source` `project` and `right_margin` 100 from `project`, and jdtls indents with two
  spaces; with an `.editorconfig` `[*.java] indent_size = 3` it indents with three from
  `editorconfig`; without `codeStyleConfig.xml` with four from `ide`; `idea_project` answers
  `applies`. Without jdtls the formatting part is skipped (it fails with `FERRIS_IDEA_REQUIRE_TOOLS`).
- **AK-10 (v6, behaviour 8)** `editor_view::tests`: the editor's right margin column follows
  `.editorconfig`, else the scheme, else Code Style; Reformat's indentation takes the scheme over
  the detected one and `.editorconfig` over both.

## Out of scope
Code Style's wrapping, spaces, blank lines and arrangement options; editing the project's code
scheme in Settings or writing `.idea/codeStyles` (v6 reads and applies it only), named IDE schemes
the project prefers (they live in the reference IDE's settings folder), the scheme's continuation
indent and its options for languages without a Code Style row; font weight; the reference's other Git options (credential helper,
commit checks).

## Known deviations from the reference
The font list holds the bundled font and known monospaced families, not every installed font.
Code Style holds indentation and the margin only, and there is no switch for detecting the
file's indentation (behaviour 7 is always on); formatting details come from the language's
formatter. Protected branches are checked by Ferris IDEA before Git is asked to push.
**v6:** the project's scheme applies only to indentation and the right margin. Which level gave the
indentation and the margin is part of `format_file`'s answer (IMP-0510); the reference only warns
in its settings that detected indents and EditorConfig may override the scheme. Reformat Code in the
window passes the scheme's indentation to the formatter where the reference's detection would not
apply either; while editing, the detected indentation still comes first.
