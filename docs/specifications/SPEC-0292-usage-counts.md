# SPEC-0292: Usage counts of declarations

- **Version:** 8 · **Date:** 2026-11-17 (v8: members in every language, usage lenses of the server, visible declarations first within a budget, a cache per text, asked again after a failure or an older answer, a click opens Find Usages, MCP `complete`, `source` and a line range; behaviours 10–17, OBS-0079); 2026-11-13 (v7: a declaration a server lists among its own usages is not counted, behaviour 2; v6: Kotlin counts members, inheritors and implementations, OBS-0035 L1; v5: Java counts members too; v2: the count at the end of the declaration's line by
  default, the row above as a setting; OBS-0013 item 3; v3: the author of a declaration as a further hint; OBS-0022, OBS-0014 item 7;
  v4: the author hint in detail and implemented, behaviours 7a–7f)
- **Sources:** OBS-0011 step 2, finding 3 (in an opened Java file the reference shows a grey count of
  usages on its own row above the record; the class without usages and the methods showed none in that
  file; Ferris IDEA shows nothing), STD (LSP 3.17 `textDocument/documentSymbol` with symbol kinds and
  `textDocument/references` with `includeDeclaration`), OSS (scheme key
  `INLAY_TEXT_WITHOUT_BACKGROUND`), SPEC-0024 (code insight), SPEC-0083 (rows and folds); v2:
  OBS-0013 item 3 (the same reference build showed the count at the end of the declaration's line;
  both placements were seen, so the reference offers both, and the later session decides the default);
  v8: OBS-0079 (Islands Light side by side, seen beside F-085: the reference showed `N usages` at the
  declarations of an opened Java file, Ferris IDEA none; the same file counted headless in this version's
  work gave the expected counts, so the window lost them), STD (LSP 3.17 `textDocument/codeLens`,
  `codeLens/resolve`), OSS (rust-analyzer's reference lenses and its `rust-analyzer.showReferences`
  command with uri, position and places as arguments), SPEC-0293 (Find Usages), SPEC-0278 (safe mode)
- **Feature matrix rows:** F-013, F-032, F-034 (v8: the click on a count opens Find Usages)
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
   takes no extra row, and a click on the count opens Find Usages (behaviour 15; up to v7 it acted
   like a click after the end of the line). [GEN, OSS, OBS-0013]
4a. **Row above (v1 look, setting):** with the position set to *above*, the count is a row of its own
   above the declaration's line, indented like the declaration. The row has no line number, the caret
   skips it, a click on its text opens Find Usages (behaviour 15) and a click beside the text puts the
   caret at the start of the declaration's line, and it takes part in
   scrolling like a line. A folded declaration keeps its row; a row whose declaration is hidden by a
   fold is hidden too. Kept because the reference was seen with both placements (OBS-0011, OBS-0013).
   [GEN, OBS-0011]
5. **Settings:** Editor › General › Code Insight › *Show usages of declarations* (named *Show usages
   of types* up to v7; on by default); off, the
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

10. **v8 – Members in every language.** For the files no language module of its own counts (all but
   Java, Kotlin, JavaScript and TypeScript), the counted declarations are the types of behaviour 1
   and, from the same document symbols, the methods, functions, constructors, fields, properties and
   enum members (symbol kinds 6, 12, 9, 8, 7, 22) that stand in the file or in a type — not those
   inside a function or method (locals, nested functions) and no variables or constants. At most 200
   per file, in document order. As in behaviour 2, a declaration without usages shows nothing here:
   an empty answer of a server that is still indexing would otherwise read as `no usages`. [STD, GEN]
11. **v8 – Usage lenses.** Where the file's server announces `codeLensProvider`, its code lenses are
   asked first. A lens is a usage lens when its command — after `codeLens/resolve` for a lens without
   one, if the server resolves lenses — shows references and carries their places as its third
   argument (uri, position, places; `editor.action.showReferences` and the servers' own commands
   named after it: `rust-analyzer.showReferences`, `java.show.references`). Such a lens gives the
   count of the declaration on its line: its places counted once each (behaviour 2). Declarations
   without a usage lens are asked with `textDocument/references` as before; a server without usage
   lenses is asked only that way. rust-analyzer offers reference lenses only on request: Ferris IDEA
   switches them on for types, methods, traits and enum variants (`lens.references.*`) and announces
   the `rust-analyzer.showReferences` command to it; the lenses read code and run nothing, in safe
   mode too. Java, Kotlin, JavaScript and TypeScript keep their own counting (behaviours 8, 9,
   SPEC-0320), because it adds what is below a declaration. [STD, OSS]
12. **v8 – Budget (performance rule).** Counting never asks about a whole large file at once:
   - The editor asks first about the declarations on its visible lines and 20 lines above and below
     them, with a budget of 15 s; those counts are shown as soon as they arrive.
   - Then it asks about the rest of the file, nearest to the visible lines first, with a budget of
     60 s, leaving out the declarations already counted for this text; the counts of both questions
     are shown together.
   - Every question counts at most 200 declarations. A question whose budget ends gives the counts
     found so far and marks itself incomplete; the declarations not reached show nothing until the
     next question (save, reopening). One question per file runs at a time (behaviour 3), and the
     questions run in the background, never on the window's thread.
   [GEN]
13. **v8 – Cache.** The window keeps the last counts of each file together with the text they were
   counted for, for at most 64 files (the longest unused is dropped first). A file opened again with
   the same text shows its kept counts at once and is counted again in the background, because other
   files may have changed meanwhile. Counts are never kept on disk. [GEN]
14. **v8 – Asked again.** A question that failed because the server was still starting or did not
   answer in time is asked again after the growing pauses of semantic tokens (SPEC-0290 v3
   behaviour 1a, five times at most); a missing server is not asked again. An answer for an older
   text than the editor's is not shown (behaviour 3); the file is asked again once its text has not
   changed for 2 s. Up to v7 both cases waited for the next save, which left a freshly opened file
   without counts when its first question came too early (the likely reason for OBS-0079). [GEN]
15. **v8 – Click.** A click on a count — at the end of the line or on its row above — puts the caret on
   the declaration's name and opens Find Usages for it (SPEC-0293 behaviour 1), as `FindUsages` (⌥F7)
   there would. The count is not part of the text otherwise (behaviour 4). [GEN, SPEC-0293]
16. **v8 – Safe mode and trust.** Counting asks only questions that read code: document symbols,
   references, code lenses, type hierarchy. It runs no project code itself. Counts therefore appear in
   safe mode wherever the file's server runs there, with its safe options (SPEC-0278 behaviour 9),
   and not where that server is not started in safe mode. Remote projects have no counts
   (SPEC-0318). [GEN]
17. **v8 – Agents.** MCP `usage_counts` takes optional `from_line` and `to_line` (0-based, inclusive):
   only the declarations on those lines are counted, as the editor's first question does. Its answer
   adds `complete` (false when the budget ended before every chosen declaration was counted) and, per
   declaration, `source`: `references` or `code lens`. The kinds of behaviour 10 are `method`,
   `function`, `constructor`, `field`, `property` and `enum member`. [ADR-0008]

## Acceptance criteria
- **AK-1** Selection and counting: from a document symbol tree with nested types, methods and fields,
  only types are chosen; references at the same place count once; the label reads `1 usage` /
  `3 usages` (unit tests).
- **AK-2** Rows above (behaviour 4a): with counts above lines 2 and 6, then a fold over lines 6–9 and
  one hiding line 2, the row of every line, the line of every row (a count row maps to its
  declaration's line), the row count and the scroll limit follow; edits move counts and an edit on a
  declaration's line drops its count (unit tests).
- **AK-3** In a real window (`ferris-idea ui --selftest`): in the Java insight file with jdtls
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
  `authors::tests` in `ferris-idea-vcs`); Java declarations with the lines of their nodes (unit test
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

- **AK-14 (v8)** Members in every language: from a symbol tree with a type, its method, field and
  constructor, a top-level function, a variable, a constant and a function with a nested function and
  a local, the type, method, field, constructor and top-level function are chosen, the rest not
  (unit test `usages::tests::members_of_every_language`).
- **AK-15 (v8)** Budget order: declarations on the visible lines and their margin come first in
  document order, the rest by distance to them; already counted ones are left out; with *visible
  only* the rest is dropped; at most 200 (unit test `usages::tests::visible_declarations_come_first`).
- **AK-16 (v8)** Usage lenses: a resolved lens with a show-references command gives the count of its
  places, once each; a lens with another command (run, implementations) or without places gives none
  (unit test `usages::tests::reference_lenses_give_counts`). With a real rust-analyzer in a Cargo
  crate: a struct and a function used once get their counts from lenses (`source` `code lens`), a
  field without a lens from the references (`references`, 3), an unused function shows nothing, and
  a line range counts only the function on it (`crates/languages/tests/it/rust_analyzer.rs`
  `usage_counts_from_reference_lenses`, skipped without rust-analyzer). rust-analyzer's lens leaves
  the `impl` header of a type out (its implementations lens names those).
- **AK-17 (v8)** Cache and merging: the counts of a text are found again for the same text and not
  for another, the 65th file drops the longest unused one, the counts of the first and the second
  question are shown together and a recount replaces both (unit tests `usage_counts_model::tests`).
- **AK-18 (v8)** MCP: `usage_counts` with `from_line`/`to_line` counts only the declarations in those
  lines and answers `complete` and `source` (protocol test `code_insight_tools`, with clangd).
- **AK-19 (v8)** Window self-test step (written, run by the coordinator): in the Java insight file, a
  click on the record's count opens Find Usages for `Pair`.

## Out of scope
Counts of members and of inheritors in languages other than Java, Kotlin, JavaScript and TypeScript, problem hints of
the reference's code vision, authors of fields and variables. (v8) `no usages` for members of
languages without a module of their own (behaviour 10); counting declarations the view is scrolled to
after a question ran out of budget before the next save (behaviour 12).

## Known deviations from the reference
Counts come from the language server and refresh on save rather than while typing. Whether the
reference also shows counts above methods with usages in other files is an open question for the next
side-by-side session; this version follows the file observed. Which of the reference's settings
switches between the two placements was not looked up; Ferris IDEA's switch is its own.
(v4) Which rule picks the reference's main author (most lines or the last change) was left open by
OBS-0022, where both fell together; Ferris IDEA takes the most lines. The reference leaves the author out
on folded one-line declarations; Ferris IDEA shows it there as well (IMP-0330). The person symbol is
Ferris IDEA's own drawing.
(v6, OBS-0035) Kotlin: the reference names what is below an interface or an abstract class "implementations"
and below a sealed class "inheritors"; Ferris IDEA says inheritors for every type, as for Java and TypeScript, and
implementations or overrides for members. Unused Kotlin declarations show no count where the reference shows
none either, but for a different reason (see behaviour 9); the unused colour for private ones comes from
SPEC-0140 behaviour 4.
(v8) Where the reference puts its counts (row above or end of line) and which declarations get one in
languages other than Java was not observed beyond OBS-0011, OBS-0013, OBS-0027, OBS-0035 and OBS-0079;
behaviour 10 follows what Java shows. A click on the reference's count was not observed; Ferris IDEA
opens its Find Usages window (behaviour 15). The visible-first order, the budget and the cache
(behaviours 12–13) are Ferris IDEA's own.
