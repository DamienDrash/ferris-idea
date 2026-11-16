# SPEC-0030: Code folding

- **Version:** 3 · **Date:** 2026-11-14 (v3: custom regions, placeholders, the outermost
  container, where a block starts and ends, the import block of Java and Kotlin, SPEC-0083 v8;
  v2: Kotlin imports are a region)
- **Sources:** GEN (editors collapse blocks, bodies, literals, multi-line comments and import groups to one
  line; F-014), STD (Tree-sitter syntax trees, SPEC-0027; LSP `FoldingRange` kinds `comment`, `imports`,
  `region`)
- **Feature matrix rows:** F-014

## Purpose
Offer collapsible regions for every supported language from the syntax tree, consistent after every edit.

## Behaviour
1. Regions come from the SPEC-0027 syntax tree of the document; languages without a grammar have none.
   [STD]
2. A region is a named node spanning at least two lines that is a block: it starts with `{`, `[` or `(`
   and ends with the matching bracket, or is an indentation block (Python `block`, folded from its header
   line such as `class A:`), or an element with children (HTML). Its fold keeps the start line visible and hides up to the end line. [GEN]
3. Comments spanning two or more lines, and runs of two or more consecutive comment lines, are regions of
   kind `comment`. [GEN]
4. Two or more consecutive import/use/include statements form one region of kind `imports`. [GEN]
   **v2:** Kotlin's import statements count too (the Kotlin grammar names the statement like its keyword,
   which v1 did not recognise). [STD]
5. Regions are sorted by start line, then by size (outer first); of several regions starting on the same
   line only the outermost is kept, so one click folds one thing. [GEN]
6. MCP tool `folding_ranges` (path): regions with 0-based start and end line and kind. [ADR-0008]
7. **(v3)** What SPEC-0083 v8 needs of the regions:
   - A bracketed block whose opening bracket stands behind other text on its line starts on that
     line, also when its parent node starts above (an annotation above a signature, the `else`
     of an `if`); a bracket alone on its line and an indentation block still start on the line
     of their header (behaviour 2).
   - A block that ends on the line where another block starts (`} else {`) ends on the line
     before; when that leaves one line it is no region.
   - In a Java or Kotlin file the imports are one region from the first to the last import with
     the blank lines and comments between their groups (SPEC-0083 behaviour 9), not one per
     run.
   - Two comment markers make a region of kind `region` (SPEC-0083 behaviour 19); on its first
     line it comes before the other regions (behaviour 5), and marker lines are no part of a run
     of comments (behaviour 3).
   - Every region carries `collapsed_text`, the placeholder shown while it is folded, and, where
     the placeholder stands for the rest of the first line, `keep`, the number of characters of
     that line that stay (SPEC-0083 behaviour 21); `doc: true` marks a documentation comment
     (behaviour 20) and `outermost: true` the outermost container of the file (behaviour 18).
     `folding_ranges` returns them. [GEN, OBS-0044 through SPEC-0083 v8, ADR-0008]

## Acceptance criteria
- **AK-1** Rust: function body, struct body, multi-line array, block comment, `use` group; single-line
  blocks give no region.
- **AK-2** Python: function and class bodies (indentation blocks), consecutive `#` comments, imports.
- **AK-3** JSON objects/arrays and HTML elements fold; one region per start line.
- **AK-4** After an edit the regions equal those of a fresh document.
- **AK-5** The MCP tool returns the regions of a workspace file.
- **AK-6 (v2)** Two consecutive Kotlin imports are one `imports` region; the `import` keyword inside a
  Python import statement is not taken for a statement (part of
  `syntax::tests::java_and_kotlin_import_blocks`).
- **AK-7 (v3)** The regions of the Java file of OBS-0044 with their placeholders, an `if` and its
  `else` as two regions, a method under an annotation, custom regions in four languages
  (`folding::tests::outline_of_a_java_file`, `custom_regions_from_comments`); `folding_ranges`
  returns the new fields (`protocol.rs`: `folding_ranges_of_a_file`,
  `folding_actions_and_custom_regions`).

## Out of scope
Folding state persistence and rendering (UI). (Custom `region`/`endregion` comments are regions
since v3. Call-argument lists over several lines have been regions since v1 through behaviour 2,
although this section said otherwise.)

## Known deviations from the reference
Region choice follows the syntax tree; the reference's per-language rules are compared in the guided
session.
