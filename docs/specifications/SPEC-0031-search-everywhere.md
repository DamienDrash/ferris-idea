# SPEC-0031: Search everywhere — files, actions, symbols

- **Version:** 6 · **Date:** 2026-11-16 (v6: Java records and enums are classes and symbols, nested ones too, behaviour 8a, AK-13, OBS-0071 G1; deviation IMP-0442); 2026-11-12 (v5: files are the project's own sources, the rest on purpose, decision of the Project Owner of 2026-11-12; behaviour 12, AK-12); 2026-11-06 (v2: titles of tool windows and the theme switcher, words in any order; hand tests 23, 28, 150, 181; v3: menu labels as titles, hand tests 39, 199; v4: word-start matching, classes and text, `name:line`, recent files first, calculator, containers and parameters, OBS-0023)
- **Sources:** GEN (one input finds files, classes/symbols and actions by name; abbreviations and camel
  humps such as `GoDe` find `GotoDeclaration`; F-016), STD (Tree-sitter `tags.scm` queries with
  `@definition.*` and `@name` captures, shipped with the grammar crates, MIT), SPEC-0001 (workspace files),
  SPEC-0027 (grammars), SPEC-0028 (action ids and shortcuts)
- **Feature matrix rows:** F-016

## Purpose
Find anything in the workspace or the IDE by typing a few characters of its name.

## Behaviour
1. Fuzzy matching: every query character must appear in the candidate in order, ignoring case (spaces in
   the query are ignored). The score rewards matches at the start of the candidate, at word starts (after
   `/ _ - . :` or a space, or an upper-case letter after a lower-case one), consecutive matches and exact
   case; it penalises gaps and long candidates. A match reports the matched char positions so the UI can
   highlight them. A query of several words also matches when each word matches on its own in
   another order (`attach profiler to process` finds `Profiler Attach To Process`); such a match
   scores below one in the typed order. [GEN]
2. Among equal scores, shorter candidates and then alphabetical order come first. [GEN]
3. Files: workspace files (SPEC-0001); the file name is matched and wins over a match in the directory
   part; a query containing `/` is matched against the whole relative path. [GEN]
4. Actions: the action ids of the keymap (SPEC-0028) shown as words (`GotoDeclaration` →
   `Goto Declaration`), matched on the id, with their shortcut in the platform's display. Actions
   whose menu title differs carry that title and its group (`QuickChangeScheme` → `Quick Switch
   Scheme…`, `ChangeLaf` → `Theme…`, `CommentByLineComment` → `Comment with Line Comment`, `About`
   → `About Ferris IDEA`); tool window actions (`Activate…ToolWindow`, `….ToolWindow`) are titled
   with the window's name (`npm`, `Containers`) in the group `Tool Windows`. (v3) Any other action
   that has a place in the menu bar (SPEC-0124) is titled with its menu label and grouped under its
   menu or submenu (`SplitVertically` → "Split Right", View); the id still matches. [GEN]
5. Symbols: definitions found by each grammar's tags query (classes, functions, methods, interfaces,
   types, modules, constants) in workspace files of C, Go, Java, JavaScript, Python, Rust and TypeScript up to
   1 MiB; each with name, kind, file and 0-based line. [STD]
6. Results are grouped by category (files, symbols, actions) with at most `limit` per category (default
   20), best first. [GEN]
7. MCP tool `search_everywhere` (query, optional categories and limit). [ADR-0008]

**v4 (OBS-0023 L1–L5, L12–L14, L16).** Behaviour 1 stays for Recent Files and the other fuzzy lists;
Search Everywhere matches as follows.
1a. **Word starts:** the query is cut into fragments of consecutive candidate characters, and every
   fragment starts at a word start (the first character, after `/ _ - . :` or a space, an upper-case
   letter after a lower-case one, a letter after a digit); words of the candidate may be skipped.
   `order` finds `OrderService`, `createOrder`, `describe_order`, `calculateOrderTotal`, not
   `ProductService`; `OrSe` finds `OrderService` only; `calculatetotal` finds `calculate_total` and
   `calculateOrderTotal`. A `*` lets the next fragment start anywhere; a space asks for a new word;
   a separator in the query (`.`, `/`, `-`, `_`, `:`) is matched by the same separator. Case is
   ignored, exact case, the start of the candidate, consecutive letters and an exact whole name score
   higher; letters that run on into the next hump (`applyDisc` in `applyDiscount`) score above a jump
   over a separator (`apply_discount`). Several words also match in any order (as behaviour 1). [GEN; OBS-0023 item 2]
4a. **Shortcuts and titles:** the first keyboard shortcut or gesture is shown, never a mouse shortcut;
   an action without a menu title whose id starts with `Editor` reads without it
   (`EditorDeleteToWordEnd` → "Delete To Word End"). [GEN; OBS-0023 item 5]
5a. **Containers and parameters:** each symbol carries where it lives — the Java/Kotlin package after a
   `java`/`kotlin` source folder (`lab.shop.model`), the dotted module of a Python file
   (`py.shop.order`), else the file's path — and functions and methods their parameter list from the
   definition's line: types for Java (`(String, long)`), names elsewhere (`(id, totalCents)`, Python
   without `self`/`cls`); none when the list does not close on the line. A query with `.` also
   matches `container.name`, so `model.Ord` finds `Order` in `lab.shop.model`. [GEN; OBS-0023 item 2]
6a. **Recent files first:** hits in the recently opened files (SPEC-0069) score higher, the latest
   most; an empty query lists the recent files (files category only). The text category searches them
   first. [GEN; OBS-0023 items 1, 4]
7a. **`name:line[:column]`:** a query ending in `:N` or `:N:M` (1-based) searches for the part before
   it and reports the 0-based line and column, where a chosen file or symbol opens. [GEN; OBS-0023
   item 3]
8. **Classes:** the category `classes` keeps class-like definitions (class, interface, enum, struct,
   trait, type, record, union) with the same matching. [GEN]
8a. **Java records and enums (v6):** Java's definitions query names classes, interfaces and methods
   but neither records nor enums; they are added from the syntax tree, top-level and nested alike,
   a record as a `class` and an enum as an `enum`, with the same container and matching as any
   class. So the record `Item` declared inside `Inventory` is found under classes and symbols. [GEN;
   OBS-0071 G1]
9. **Text:** the category `text` lists lines of workspace files containing the query, with the find
   options match case, whole words and regular expression (SPEC-0029), one row per line with the
   match's column and length; binary files and files over 10 MiB are skipped. [GEN; OBS-0023 item 6]
10. **Calculator:** when files and actions are searched (the All tab) and the query is an arithmetic
   expression with at least one operator (`+ - * / % ^`, parentheses, decimals), the result carries its
   value (`12*7` → `84`; division by zero gives none). [GEN; OBS-0023 item 5]
11. The MCP tool takes the categories `files`, `classes`, `symbols`, `actions`, `text` (default all but
   `classes`), the text options, reads the window's recent files, and returns `calculator`, `line`
   and `column`. [ADR-0008]
12. **Files are the project's own sources (v5):** the files category (Search Everywhere's Files and
   All tabs, Go to File, the MCP tool, the host's `search.files` and `search.everywhere`) lists the
   files that are the project's own by the rule of SPEC-0148 v4 behaviour 7 (`OwnSources`), as
   Rename and Find in Files do: not virtual environments, installed dependencies, build output,
   version control and IDE data, excluded folders, files behind a symbolic link, files Git ignores
   and does not track.
   - **On purpose:** "Include non-project items" in the window (SPEC-0067 v6 behaviour 13), MCP
     `include_non_project: true`, host `include_non_project: true` list every file but version
     control data and `.ferris-idea`.
   - **Always:** files open in an editor and recently opened files (SPEC-0069) that exist are
     listed whatever they are, so a file once opened from `.venv` is found again.
   - **Said in one line:** what was left out, `2 folders and 1 file not listed (libraries and
     ignored files)`; the MCP tool adds how to list them (`not_listed`).
   - **Not changed:** classes, symbols and text keep their lists; only files are limited.
   - **Cost:** in a tree with 400 own files and 50 000 in `.venv`, listing and searching the files
     took 0.24–0.26 s and 0.84–1.0 s before, 3.5–10 ms and 5–19 ms after (measured on the Linux
     server, debug build). [Decision of the Project Owner of 2026-11-12; GEN]

## Acceptance criteria
- **AK-1** Matching and scoring: subsequence rule, camel humps (`GoDe`, `gtdcl`), word starts beat inner
  matches, consecutive beats scattered, exact case bonus, positions reported, non-matches rejected.
- **AK-2** Ranking ties: shorter first, then alphabetical.
- **AK-3** Symbols from tags queries for Rust, Python and TypeScript samples (name, kind, line).
- **AK-4** Workspace search: file-name match beats directory match, `/` queries, symbols across files,
  actions with shortcut, per-category limit.
- **AK-6** (v2) Titles and word order: "theme", "quick switch scheme", "npm", "comment with line
  comment", "line comment with" and "attach profiler to process" find the right action first.
- **AK-5** The MCP tool returns grouped results and honours the category filter.
- **AK-7 (v4)** Word starts, `*`, separators, positions (unit test `fuzzy::tests::humps_need_word_starts`);
  `name:line`, calculator, containers, parameters, classes, text, recent files first, empty query,
  mouse shortcuts hidden, `Editor` prefix (unit test
  `everywhere::tests::v4_categories_lines_recent_and_calculator`).
- **AK-12 (v5)** Files of the own sources; recent files in any case and a gone one not; symbols not
  limited (unit test `everywhere::tests::the_files_category_searches_its_own_list_and_recent_files`).
  In a project with `.venv`, `node_modules`, a file `.gitignore` ignores and an own file: MCP
  `search_everywhere` lists the own file and says `2 folders and 1 file not listed …`, with
  `include_non_project` all four, a recent file from `.venv` by default, classes from `.venv`
  either way (`protocol.rs::search_everywhere_lists_the_projects_own_files`); the host's
  `search.files` and `search.everywhere` the same, a recent path outside the project refused
  (`crates/host/tests/it/host.rs::go_to_file_on_the_host_lists_the_projects_own_files`).

- **AK-13 (v6)** A Java file with a top-level enum, a top-level record and a record nested in a
  class: the definitions list all three with kinds `enum`, `class`, `class` (unit test
  `syntax::tests::java_records_and_enums_are_definitions`); MCP `search_everywhere` finds the nested
  record under `classes` and `symbols` and the enum under `classes`
  (`protocol.rs::search_everywhere_finds_java_records_and_enums`).

## Out of scope
Usage-based ranking beyond recent files, symbols from language servers (SPEC-0024 covers one file),
indexes for very large workspaces, libraries and the JDK as non-project items (the files of the
project folder that are not its own sources stand for them, behaviour 12).

## Known deviations from the reference
Ranking weights are our own; the reference's order for the same query is compared in the guided session.

- **(v6) Symbols with their names (IMP-0442).** The reference's symbol search over MCP returns only the
  file and the range of a declaration; the MCP tool returns name, kind, container, line and parameters,
  so an agent knows what it found without reading the file (OBS-0071).
