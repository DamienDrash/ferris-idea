# SPEC-0292: Usage counts of declarations

- **Version:** 7 · **Date:** 2026-11-13 (v7: a declaration a server lists among its own usages is not counted, behaviour 2; v6: Kotlin counts members, inheritors and implementations, OBS-0035 L1; v5: Java counts members too; v2: the count at the end of the declaration's line by
  default, the row above as a setting; OBS-0013 item 3; v3: the author of a declaration as a further hint; OBS-0022, OBS-0014 item 7;
  v4: the author hint in detail and implemented, behaviours 7a–7f)
- **Sources:** OBS-0011 step 2, finding 3 (in an opened Java file the reference shows a grey count of
  usages on its own row above the record; the class without usages and the methods showed none in that
  file; IDECraft shows nothing), STD (LSP 3.17 `textDocument/documentSymbol` with symbol kinds and
  `textDocument/references` with `includeDeclaration`), OSS (scheme key
  `INLAY_TEXT_WITHOUT_BACKGROUND`), SPEC-0024 (code insight), SPEC-0083 (rows and folds); v2:
  OBS-0013 item 3 (the same reference build showed the count at the end of the declaration's line;
  both placements were seen, so the reference offers both, and the later session decides the default)
- **Feature matrix rows:** F-013, F-032
- **Script files (2026-11-09):** see SPEC-0320, which extends this spec for JavaScript and TypeScript.

## Purpose
See at a glance whether, and how often, a type is used, without starting Find Usages.

## Behaviour
1. **Which declarations:** types — symbols of kind class, interface, enum, struct (records come as
   classes or structs) — of the file's document symbols, nested ones included, at most 50 per file.
   [STD]
2. **Count:** for each, the server's references without the declaration, counted once per place
   (file, line, character). A type without usages shows nothing. (v7) A server may list the
   declaration although it was asked not to (`includeDeclaration: false`; vscode-css-language-server
   does: every rule of a stylesheet had "1 usage", itself); a place that is the declaration's own
   position is therefore not counted. [STD, OSS]
3. **When:** after the file is opened and after each save, with the language server pool
   (SPEC-0090); edits in between move the counts with their lines and an edit on a declaration's line
   drops its count. Counting runs in the background and never blocks typing; an answer for a text
   other than the editor's current one is ignored (the next save asks again). [GEN]
4. **Look (v2):** the count reads `1 usage` or `N usages`, in the foreground of
   `INLAY_TEXT_WITHOUT_BACKGROUND` (grey), and is not part of the text: no caret position, no
   selection, not copied. By default it stands at the end of the declaration's line, two character
   widths after the line's last character (after the placeholder when the line is folded); the line
   takes no extra row, and a click on the count acts like a click after the end of the line. [GEN,
   OSS, OBS-0013]
4a. **Row above (v1 look, setting):** with the position set to *above*, the count is a row of its own
   above the declaration's line, indented like the declaration. The row has no line number, the caret
   skips it, a click on it puts the caret at the start of the declaration's line, and it takes part in
   scrolling like a line. A folded declaration keeps its row; a row whose declaration is hidden by a
   fold is hidden too. Kept because the reference was seen with both placements (OBS-0011, OBS-0013).
   [GEN, OBS-0011]
5. **Settings:** Editor › General › Code Insight › *Show usages of types* (on by default); off, the
   editor neither counts nor draws them. Below it *Show usages above the declaration* (off by default:
   at the end of the line). Both apply to open editors at once. Also `usage_counts` and
   `usage_counts_above` in `set_ide_settings`. [GEN, ADR-0008]
6. **Agents:** MCP tool `usage_counts` (path): the counted declarations with name, kind, 0-based line
   and count. [ADR-0008]

7. **v3 – Author (planned, OBS-0022 L6):** after the usage count (or alone), a declaration —
   type, method, constructor, function — shows its main author from the file's blame at HEAD: the
   author of most of its committed lines, `+N` when N other authors wrote lines of it, and `*` when a
   line of it is changed since HEAD (`Anna Muster +1 *`). Lines not yet committed do not count. A
   click on the author opens the annotations (SPEC-0277). Setting *Show code author* under Code
   Insight (on by default), MCP `code_authors` (path → declarations with author, others, changed).
   Folded one-line declarations show it as well (the reference leaves it out there, IMP-0330).
   [OBS-0014 item 7, OBS-0022 L6] **Status (v4): implemented as in 7a–7f.**
7a. **v4 – Declarations:** the definitions of the file's tags query (SPEC-0031 behaviour 5) of kind
   class, interface, method or function, nested ones included (structs come as classes), at most
   200 per file; in Java, whose query names neither records, enums nor constructors, these are taken
   from the syntax tree as classes and methods.
   Each covers the lines of its whole node, from its first line (annotations and modifiers
   included) to its last; the hint stands on the line of its name. Files without a tags query or
   outside Git show no authors. No language server is needed. [GEN, OSS]
7b. **v4 – Main author:** the blame at HEAD (SPEC-0277 behaviour 7) is mapped onto the editor's
   text; among the declaration's lines that still carry a commit, the author with the most lines
   wins, on a tie the one whose newest commit among those lines is newer. Authors are told apart by
   name and e-mail address. `others` counts the remaining authors with at least one line. `changed`
   is true when a line change of the text against HEAD (SPEC-0064 behaviour 2) lies within the
   declaration's lines, a deletion included when its boundary lies inside them. A declaration none
   of whose lines carries a commit (all new) shows no author. [GEN]
7c. **v4 – Look:** the text reads `Name`, `Name +N`, `Name *` or `Name +N *`, in the colour of the
   usage counts, two character widths after the usage count, or after the line's end when there is
   none; with the counts set *above*, the author stays at the end of the line. A small person
   symbol drawn by the editor stands before the name (own drawing, ADR-0009). A name longer than
   24 characters is cut with `…`. The hint is not part of the text, like the counts. [GEN, ADR-0009]
7d. **v4 – When:** read in the background when a file opens, after each save and whenever the bases
   of the change markers are read again (commit, checkout, changes from outside: SPEC-0064
   behaviour 1). Between reads the hints move with the edited lines and `*` follows the change
   markers at once (SPEC-0064 behaviour 3); an edit on a hint's line keeps the hint. [GEN]
7e. **v4 – Click:** a click on the hint opens the annotations of that editor view (SPEC-0277
   behaviour 1); while they are shown a further click closes them, as the reference's hint toggles
   the column. [OBS-0022 item 6]
7f. **v4 – Settings and agents:** *Show code author* (Editor › General › Code Insight, on by
   default) and `code_authors` in `set_ide_settings`; off, nothing is read or drawn. MCP tool
   `code_authors` (path): `declarations` with name, kind, 0-based line, `author`, `email`,
   `others`, `changed` and the `label` as drawn; an open file is read as the editor shows it. An
   untracked file is an error saying so. [ADR-0008]

8. **v5 – Java members (OBS-0027 L15):** in Java files types, methods, constructors, fields and enum constants
   are counted as in SPEC-0320 behaviours 5–6: also `no usages`, a type with project subtypes adds its
   inheritors. jdtls' method names are cut at `(`, anonymous classes and initialisers are left out. [OBS-0027 item 4, STD]

9. **v6 – Kotlin (OBS-0035 L1):** in `.kt` files the counted declarations come from the syntax tree, at
   most 200 per file: types (class, interface, object, enum), functions and properties of types and of the
   file, `val`/`var` parameters of primary constructors and enum entries; local declarations are not
   counted. The usages are the server's references as in behaviour 2 (kotlin-language-server answers
   `textDocument/references` for all of these). What is below a declaration comes from the project's
   syntax trees as in SPEC-0082 behaviour 17, because the server has neither a type hierarchy nor
   implementations: a type shows its project subtypes at every depth as `N inheritors`; an abstract
   function or property the members of subtypes that declare it as `N implementations`; another member
   that can be overridden those as `N overrides`. The label joins both (`2 usages · 2 implementations`,
   `no usages · 1 implementation`). A declaration with neither usages nor anything below shows nothing,
   as in behaviour 2: an empty answer of this server can also mean that it has not finished compiling, so
   `no usages` alone would too often be wrong. Declarations the server was not asked about within the
   time limit, or failed on, show nothing; without the server (not installed, safe mode) there are no
   counts. MCP `usage_counts` gives the kinds `abstract function`, `abstract property`, `method`,
   `function`, `property`, `enum member` besides the types', and `inheritors` for what is below.
   [OBS-0035 items 4 and 12, STD]

## Acceptance criteria
- **AK-1** Selection and counting: from a document symbol tree with nested types, methods and fields,
  only types are chosen; references at the same place count once; the label reads `1 usage` /
  `3 usages` (unit tests).
- **AK-2** Rows above (behaviour 4a): with counts above lines 2 and 6, then a fold over lines 6–9 and
  one hiding line 2, the row of every line, the line of every row (a count row maps to its
  declaration's line), the row count and the scroll limit follow; edits move counts and an edit on a
  declaration's line drops its count (unit tests).
- **AK-3** In a real window (`idecraft ui --selftest`): in the Java insight file with jdtls
  installed, the nested record used twice shows `2 usages` at the end of its line and the class
  without usages shows none; the editor has no row more than the file has lines.
- **AK-4** The MCP tool lists the counted types of a C++ file when clangd is installed: the used
  struct with its count, the unused one left out (protocol test, skipped without clangd).
- **AK-5 (v2)** At the end of the line (behaviour 4): the counts add no rows, every row maps to its
  own line, the count of a line is found by its line, edits move and drop counts as in AK-2, and
  switching to *above* and back changes only the rows (unit tests); the setting is read and written
  by `set_ide_settings` (protocol test).

- **AK-8 (v4)** Main author, `+N`, ties and `*` from a blame with two authors, an edited line and a
  deletion inside a declaration; new declarations without author; labels and cutting (unit tests
  `authors::tests` in `idecraft-vcs`); Java declarations with the lines of their nodes (unit test
  `syntax::tests::declaration_spans_cover_their_nodes`); the hints move with edits and `*` follows
  the markers (unit tests `authors_model::tests`).
- **AK-9 (v4)** MCP `code_authors` on a repository with two authors: main author, `others`, and
  `changed` after an edit on disk; an untracked file is refused; `set_ide_settings` writes
  `code_authors` (protocol test `code_authors_tool`).
- **AK-10 (v4)** Window self-test step (written, run later): in the Git fixture's file a
  declaration shows its author at the end of its line, and `*` after a line of it is typed into.
- **AK-11 (v5)** Unit test `java_marks::tests::counted_declarations_of_a_java_file`; with a real jdtls: unused member `no usages`, class with inheritors (`java_structure.rs`: `gutter_marks_and_member_counts`).

- **AK-12 (v6)** Kotlin declarations and what is below them on the shapes of OBS-0035: `Animal` 4 inheritors
  across two packages, abstract `sound()` 3 implementations, `Pet.owner` 2, sealed `Shape` 2, file-level
  function and property, enum entries (unit test
  `kotlin_structure::tests::counted_declarations_with_inheritors`); the labels `1 implementation`,
  `3 implementations`, `3 overrides` (`usages::tests::counts_places_once_and_labels`); with a real
  kotlin-language-server: `sound()` at least two usages and `· 2 implementations`, `Animal`, `Pet` and
  `Shape` two inheritors each, `owner` without usages but one implementation, `total` in another file one
  usage, `main` nothing (`crates/languages/tests/it/kotlin_structure.rs::kotlin_usage_counts_with_the_real_server`,
  skipped without the server).

- **AK-13 (v7)** `usages::tests::the_declaration_itself_is_no_usage`: of three places one is the
  declaration's own and is left out; a place on another line or in another file stays. Headless with
  the CSS server: a stylesheet of nine rules lists two counts (a class used in a second rule)
  where it listed nine.

## Out of scope
Counts of members and of inheritors in languages other than Java, Kotlin, JavaScript and TypeScript, problem hints of
the reference's code vision, authors of fields and variables, clicking the count to list the usages (planned with Find Usages).

## Known deviations from the reference
Counts come from the language server and refresh on save rather than while typing. Whether the
reference also shows counts above methods with usages in other files is an open question for the next
side-by-side session; this version follows the file observed. Which of the reference's settings
switches between the two placements was not looked up; IDECraft's switch is its own.
(v4) Which rule picks the reference's main author (most lines or the last change) was left open by
OBS-0022, where both fell together; IDECraft takes the most lines. The reference leaves the author out
on folded one-line declarations; IDECraft shows it there as well (IMP-0330). The person symbol is
IDECraft's own drawing.
(v6, OBS-0035) Kotlin: the reference names what is below an interface or an abstract class "implementations"
and below a sealed class "inheritors"; IDECraft says inheritors for every type, as for Java and TypeScript, and
implementations or overrides for members. Unused Kotlin declarations show no count where the reference shows
none either, but for a different reason (see behaviour 9); the unused colour for private ones comes from
SPEC-0140 behaviour 4.
