# SPEC-0150: Font family, Code Style and Version Control settings

- **Version:** 4 · **Date:** 2026-11-15 (v4: the protected branches field in the semantic tree, AK-6; v3: SCSS and Less get rows of their own with two spaces, behaviour 2, OBS-0036 FM1); 2026-11-10 (v2: ligatures checkbox and MCP field)
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
   from the bundled JetBrains Mono and the installed monospaced families IDECraft knows (Menlo,
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

## Out of scope
Code Style's wrapping, spaces, blank lines and arrangement options; per-project code style
schemes; font weight; the reference's other Git options (credential helper,
commit checks).

## Known deviations from the reference
The font list holds the bundled font and known monospaced families, not every installed font.
Code Style holds indentation and the margin only; formatting details come from the language's
formatter. Protected branches are checked by IDECraft before Git is asked to push.
