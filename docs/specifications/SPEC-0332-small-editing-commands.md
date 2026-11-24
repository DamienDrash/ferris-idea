# SPEC-0332: Small editing commands

- **Version:** 2 · **Date:** 2026-11-14 (v2: Copy Reference, the path with a line number and the
  path from a source root; the copy popup for the project tree's selection and for a tab;
  behaviours 13–13f, AK-11 to AK-15; v1 2026-11-13)
- **Sources:** OSS — the open platform's actions, `JetBrains/intellij-community` at the pinned
  commit `d4018fb` of APACHE_PORTING.md (PROVENANCE.md: logic template, no code):
  `platform/platform-impl/src/com/intellij/openapi/editor/actions/AbstractPermuteLinesHandler.java`,
  `SortLinesAction.java` and `ReverseLinesAction.java` (which lines, their new order, caret and
  selection afterwards), `TransposeAction.kt` (characters around the caret, the end of a line,
  several selections), `AddCaretPerSelectedLineAction.java` (the lines of a selection, the primary
  caret), `ConvertIndentsActionBase.java`, `ConvertIndentsToSpacesAction.java` and
  `ConvertIndentsToTabsAction.java` in the same folder with
  `platform/analysis-impl/src/com/intellij/openapi/editor/ConvertIndentsUtil.java` (the width of an
  indentation and how it is rebuilt), `platform/lang-impl/src/com/intellij/ide/actions/CopyPathProvider.kt`
  and `CopyReferencePopup.java` (what each entry of the copy popup yields; several items give one
  line each; an entry without a result is left out), `CopyReferenceAction.java` and
  `CopyReferenceUtil.java` in the same folder (Copy Reference: the names of the elements, else the
  file's name with the caret's line), `platform/refactoring/src/com/intellij/ide/actions/FqnUtil.java`
  and `VirtualFileQualifiedNameProvider.java` (the name of a file: a provider's, else the path
  below the project's base folder, else the whole path),
  `java/java-impl/src/com/intellij/ide/actions/JavaVirtualFileQualifiedNameProvider.java` (a file
  of a Java module: the path below its source folder, else below the content root),
  `java/java-impl/src/com/intellij/ide/actions/JavaQualifiedNameProvider.java` (Java names: a
  type by its qualified name, a member after `#`, parameter types for an overloaded method, the
  members of anonymous and local classes),
  `python/python-code-insight/src/com/jetbrains/python/actions/PyQualifiedNameProvider.java` with
  `python/python-psi-impl/src/com/jetbrains/python/psi/resolve/QualifiedNameFinder.java` (Python
  names: classes and functions only, dotted below the module, nothing below a function),
  `plugins/kotlin/base/code-insight/src/org/jetbrains/kotlin/idea/KotlinQualifiedNameProvider.kt`
  (Kotlin names go through the Java rule on the JVM view of the declaration), the status texts'
  keys in `platform/platform-api/resources/messages/IdeBundle.properties` (read, not taken over),
  `platform/lang-impl/src/com/intellij/codeInsight/editorActions/fillParagraph/FillParagraphAction.java`
  and `ParagraphFillHandler.java` (read for behaviour 14 only); the action ids and their places in
  `platform/platform-impl/resources/idea/PlatformActions.xml`, the action texts in
  `platform/platform-resources-en/src/messages/ActionsBundle.properties`, the shortcut of
  `EditorAddCaretPerSelectedLine` in the ported keymaps (SPEC-0028). GEN (one undo step per
  command, a status message). SPEC-0057 (editor commands by action id), SPEC-0124 v9 behaviour 12
  (the menu items), SPEC-0048 v7 (source folders), SPEC-0079 (the symbols of a file as the
  language server lists them), STD: `textDocument/documentSymbol` of the Language Server
  Protocol, SPEC-0149 v4 behaviour 11 (read-only texts), SPEC-0137 and SPEC-0150 (tab
  width of a file), SPEC-0275 (copying paths from the project tree and the tabs), SPEC-0319
  (declared effects), ADR-0008. Nothing here was observed in the reference IDE.
- **Feature matrix rows:** F-010

## Purpose
Sort or reverse lines, swap two characters, put a caret at the end of every selected line, turn
indentation into spaces or tabs, and copy the path or the name of the file being edited — each by
an action with the open platform's id, from the Edit menu, from Find Action and for agents.

## Behaviour

### Common
1. **Editor commands:** `EditorSortLines`, `EditorReverseLines`, `EditorTranspose`,
   `EditorAddCaretPerSelectedLine`, `ConvertIndentsToSpaces` and `ConvertIndentsToTabs` are editor
   commands of SPEC-0057 (`perform`, `COMMANDS`). Each is one undo step; a command that changes
   nothing adds no undo step. A read-only text stays as it is and the status bar says that it is
   read-only (SPEC-0149 v4 behaviour 11); `EditorAddCaretPerSelectedLine` changes no text and works
   there. [GEN, OSS: the ids]
2. **Lines and line breaks:** a line is the text between line breaks. A command that reorders
   lines or rebuilds indentation leaves every line break where it is and as it is (`\n` or `\r\n`),
   so a file with Windows line breaks keeps them. A `\r\n` counts as one character for Transpose.
   [GEN]

### Sort Lines and Reverse Lines
3. **Which lines:** the commands act on the primary caret only. With a selection the lines are
   those from the line the selection starts on to the line it ends on; without one, all lines of
   the text. A last line on whose first column the range ends is left out: a selection that ends
   at the start of a line does not take that line, and the empty line after a final line break is
   not sorted. With fewer than two lines nothing changes and the status bar says so. [OSS,
   `AbstractPermuteLinesHandler.java`; GEN: the message — the open action is disabled instead]
4. **Order:** Sort Lines puts the lines in ascending order of their UTF-16 code units, the whole
   line compared, capitals before small letters, no locale and no numbers read as numbers; equal
   lines keep their order. Reverse Lines puts the last line first. Only the lines' texts change
   places (behaviour 2). [OSS, `SortLinesAction.java`: the natural order of strings;
   `ReverseLinesAction.java`]
5. **Caret afterwards:** with a selection, the selection then runs from the start of the first
   line to the start of the line after the last one (the end of the text when there is none), the
   caret at its end. Without a selection the caret stays in its line at its column and moves with
   that line; a caret on the empty line after the final line break stays there. Further carets
   keep their offsets. [OSS, `AbstractPermuteLinesHandler.java`; GEN: further carets]

### Transpose
6. **No caret has a selection:** for every caret, in the order of the text: a caret at the start
   of the text does nothing. A caret before the end of its line swaps the character before it with
   the character after it and moves behind both; at the start of a line the character before it is
   the line break, which changes places with the line's first character. A caret at the end of its
   line swaps the two characters before it and stays; on an empty line these are the line break
   before it and the character before that. With fewer than two characters before a caret at the
   end of a line nothing happens. A character is a Unicode scalar value, so a letter outside the
   Basic Multilingual Plane moves as a whole; a combining mark is a character of its own. [OSS,
   `TransposeAction.kt`]
7. **Every caret has a selection, and there are at least two:** the selected texts rotate: every
   selection gets the text of the selection before it, the first gets the last one's. Each caret
   selects its new text, the caret at the end. [OSS, `TransposeAction.kt`]
8. **Otherwise** — some carets with a selection and some without, or one caret with a selection —
   nothing changes. [OSS, `TransposeAction.kt`]

### Add Carets to Ends of Selected Lines
9. **Carets:** every caret is replaced by one caret without selection at the end of each line its
   selection touches, from the line the selection starts on to the line it ends on; the last line
   is left out when the selection ends at its first column and spans more than one line. A caret
   without a selection goes to the end of its line. Of the carets that replace the primary caret,
   the one on the last line is primary when the primary caret stood at the end of its selection,
   else the one on the first line. [OSS, `AddCaretPerSelectedLineAction.java`; GEN: which is
   primary when the caret stood at the selection's start]

### Convert Indents
10. **Range:** the lines the primary caret's selection touches, the line it ends on included also
    when it ends at that line's first column; without a selection all lines. [OSS,
    `ConvertIndentsActionBase.java`, `ConvertIndentsUtil.java`]
11. **Indentation:** a line's indentation is its leading spaces and tabs; a line of only spaces
    and tabs is indentation as a whole. Its width counts one column per space and, for a tab, the
    columns up to the next multiple of the tab width. The tab width is the file's
    (`.editorconfig`, else the code style of its language; SPEC-0137, SPEC-0150). `ConvertIndentsToSpaces` writes
    the width as spaces; `ConvertIndentsToTabs` writes as many tabs as the tab width fits into the
    width and the rest as spaces. A line whose indentation already reads so, and a line without
    indentation, is not touched. Text after the indentation, tabs in it included, stays. [OSS,
    `ConvertIndentsUtil.java`]
12. **Afterwards:** carets and selections stay on their text. The status bar says in how many
    lines the indentation was converted, or that it already uses spaces (tabs). [OSS: a hint with
    the number of changed lines; GEN: the wording, the status bar]

### Copy Path/Reference…
13. **The popup:** `CopyReferencePopupGroup` opens a list with one row per kind of text, each
    showing what it would copy; choosing a row copies it (behaviour 13f). Each row is also an
    action of its own that copies at once. The rows, in this order, each only when it has
    something to copy:
    - Absolute Path (`CopyAbsolutePath`),
    - File Name (`CopyFileName`),
    - Path with Line Number (`CopyPathWithLineNumber`; behaviour 13a),
    - Path from Content Root (`CopyContentRootPath`: the path below the project folder with `/`
      between the names; nothing for the project folder itself and for an item outside it),
    - Path from Source Root (`CopySourceRootPath`; behaviour 13a),
    - Copy Reference (`CopyReference`; behaviour 13b).

    An editor without a file has no popup; the status bar says so. [OSS, `CopyPathProvider.kt`,
    `CopyReferencePopup.java`, `PlatformActions.xml`, `ActionsBundle.properties`: the entries,
    their texts and their order; GEN: the project folder is the content root]
13a. **Source root and line number:** the source folders are those of SPEC-0048 (declared by a
    Maven or Gradle build file, or derived from the package lines of Java and Kotlin files), test
    and resource folders included. Path from Source Root is the path below the innermost source
    folder that holds the item; an item in no source folder, and a source folder itself, has
    none. Path with Line Number is the file's reference name, a colon and the 1-based line of the
    primary caret; it needs an editor. A file's reference name is its path below its source
    folder; without one its path below the project folder; outside the project folder its
    absolute path. [OSS, `CopyPathProvider.kt`, `FqnUtil.java`,
    `JavaVirtualFileQualifiedNameProvider.java`; GEN: Ferris IDEA has source folders only where
    SPEC-0048 finds them, which are the folders of JVM sources]
13b. **Copy Reference** (`CopyReference`, ⌥⇧⌘C on macOS, Ctrl+Alt+Shift+C elsewhere) acts on the
    active editor. It copies the qualified name of the symbol the primary caret is in
    (behaviour 13c). Where there is none — the caret outside every symbol, a language without
    naming rules, no answer from the language server within five seconds, a document of a remote
    project — it copies the file's reference name, a colon and the caret's 1-based line
    (behaviour 13a), and the status bar says so when the server gave no symbols. Without an
    editor the status bar says so. In the popup the row shows this fallback text for a language
    without naming rules and for a remote document, and no text otherwise, since the name is
    asked of the server only when the row is chosen. [OSS, `CopyReferenceAction.java`: the name,
    else file name and line; GEN: the symbol around the caret, the time limit, the row without a
    preview]
13c. **Qualified names:** the symbols of the file come from the language server's document
    symbols on the editor's text (SPEC-0079), no other request. A symbol holds the caret from the
    first column of the line of its name to its end. Of the symbols around the caret the
    innermost one that can be named counts: a type (class, interface, enum, struct, object), a
    module or namespace, a function, method or constructor, a field, property, constant or enum
    member; local variables, parameters and lambdas are passed over. A parameter list at the end
    of the server's name of a function is not part of its name. What is copied per language:

    | Language | Symbol | Copied |
    |---|---|---|
    | Java, Kotlin | type | the package of the file's `package` line, the enclosing types and the type, joined by `.`: `shop.Cart`, `shop.Cart.Line`; without a package line `Cart` |
    | Java, Kotlin | member of a type | the type as above, `#`, the member: `shop.Cart#total`; a constructor by the name the server gives it, `shop.Cart#Cart` |
    | Java, Kotlin | function that shares its name with another member of the same type | as above with the parameter list as the server names it: `shop.Cart#add(int, String)`; without a list in the server's name nothing is added |
    | Java, Kotlin | member of an anonymous class, or of a type declared inside a function | its simple name: `run` |
    | Java, Kotlin | an anonymous class itself, a type or function declared inside a function | no name (file and line) |
    | Kotlin | declaration at the top level of the file | package and name joined by `.`: `shop.total` |
    | Python | class, function or method | the module, the enclosing classes and the name, joined by `.`: `shop.cart.Cart.total`. The module is the file's path below its source folder, else below the project folder, without `.py` or `.pyi` and with `.` between the names; `__init__` names its folder. Variables and attributes are passed over |
    | Python | anything inside a function | no name (file and line) |
    | Rust | module, struct, enum, trait, type alias, function, method, constant, static, field, enum variant | `crate`, the modules of the file, the inline modules, the type and the name, joined by `::`: `crate::cart::Cart::total`. The modules of the file are its path below the `src` folder next to the nearest `Cargo.toml`, without `.rs`; `lib.rs`, `main.rs` and `mod.rs` name no module of their own, and a file directly in `src/bin` is a crate root. A file outside such a `src` folder gets no `crate` and no modules: `Cart::total`. An `impl` block counts as its type (`impl Display for Cart` is `Cart`), generic parameters left out |
    | Rust | anything inside a function | no name (file and line) |
    | every other language | — | no name (file and line) |

    [OSS, `JavaQualifiedNameProvider.java`, `PyQualifiedNameProvider.java`,
    `QualifiedNameFinder.java`, `KotlinQualifiedNameProvider.kt`: the forms for Java, Kotlin and
    Python; GEN: the Rust form, which has no counterpart in the open sources; STD: document
    symbols]
13d. **From the project tree:** while the project tree has the keyboard focus, the popup and the
    path actions act on its selection, files and folders alike, and copy one line per selected
    item, in the tree's order; an item without that kind of path gives no line, and a kind none
    of them has is no row. Path with Line Number and Copy Reference are not offered there. The
    tree's menu offers Path From Source Root next to its other paths (SPEC-0275 v15
    behaviour 1). [OSS, `CopyPathProvider.kt`: one line per element; `CopyReferenceAction.java`:
    no reference for files and folders that have only a path]
13e. **From a tab and for remote files:** a tab's menu offers Path From Source Root, Path With
    Line Number and Copy Reference for its file (SPEC-0059 v9.5 behaviour 18); the tab becomes
    the active one first, so the line is its caret's. For a document of a remote project every
    path is the one on the host (SPEC-0317): the absolute path there, and below the project's
    folder there for the content root and the reference name; it has no source folders. The text
    is not marked as remote. [OSS, `CopyPathProvider.kt`: a tab stands for its file, and no path
    is marked; GEN: host paths]
13f. **Clipboard and status bar:** a copy puts plain text on the clipboard and nothing else; the
    clipboard is not read. The status bar then names the kind and the text: its first line, cut
    after 80 characters with `…`, and how many further lines there are. An action with nothing
    to copy leaves the clipboard as it is and says why. [GEN; OSS: a status text after copying]

### Not built
14. **Fill Paragraph** (`FillParagraph`) is not offered. The open action joins the lines of the
    paragraph around the caret and then wraps them through the formatter's handling of lines
    longer than the right margin, which breaks at positions the language of the file decides and
    indents the new lines as Enter would; which text counts as a paragraph (plain text, a comment,
    a string) is likewise decided per language. That is not one rule that can be restated and
    tested here; it needs a specification of its own with the wrap positions. [OSS,
    `FillParagraphAction.java`, `ParagraphFillHandler.java`]

### Menus, Find Action, agents
15. **Menu and Find Action:** the Edit menu items are SPEC-0124 v9 behaviour 12. Find Action
    lists every action of this specification under its menu title; `CopyAbsolutePath`,
    `CopyFileName`, `CopyPathWithLineNumber`, `CopyContentRootPath`, `CopySourceRootPath` and
    `CopyReference` are listed with the group Copy Path/Reference…. `EditorAddCaretPerSelectedLine`
    (⌥⇧G) and `CopyReference` (⌥⇧⌘C) have a shortcut in the bundled keymaps, the others none.
    [OSS: texts and keymap]
16. **Effects (SPEC-0319):** `EditorSortLines`, `EditorReverseLines`, `EditorTranspose`,
    `ConvertIndentsToSpaces` and `ConvertIndentsToTabs` are declared as writing;
    `EditorAddCaretPerSelectedLine`, `CopyReferencePopupGroup`, `CopyAbsolutePath`, `CopyFileName`,
    `CopyPathWithLineNumber`, `CopyContentRootPath`, `CopySourceRootPath` and `CopyReference` have
    no effect on the project (they write the clipboard, like Copy). [GEN]
17. **Agents (ADR-0008):** the MCP tool `editor_action` has `sort_lines`, `reverse_lines`,
    `transpose`, `add_carets_to_line_ends`, `indents_to_spaces` and `indents_to_tabs` (with
    `tab_size`, 1–32, default 4) — the result holds the text, the carets and, for the two
    conversions, `changed_lines` — and `copy_absolute_path`, `copy_file_name` and
    `copy_content_root_path`, `copy_source_root_path`, `copy_path_with_line` and `copy_reference`,
    whose result holds the text the action would copy in `copied` (the line and the symbol are
    those of the first caret; `copy_source_root_path` fails for a file in no source folder).
    `copy_reference` asks the language server for the saved file's symbols and adds `reference`,
    `"symbol"` or `"line"`, and after a failed request the reason in `symbols_unavailable`. Like
    every action of that tool they change neither the file nor the clipboard. [GEN]

## Acceptance criteria
- **AK-1** Sort Lines and Reverse Lines: a selection of whole lines, a selection ending at a line
  start, no selection with and without a final line break, a single line (nothing changes, a
  message), `\r\n` kept, capitals before small letters and UTF-16 order for letters outside the
  Basic Multilingual Plane, the caret following its line, one undo step
  (`small::tests::sort_*`, `reverse_*`).
- **AK-2** Transpose: in a line, at the end of a line, at the start of the text, at the start of
  a line and on an empty line, with `\r\n`, with a letter outside the Basic Multilingual Plane,
  several carets, rotation of several selections, nothing for mixed carets and for one selection
  (`small::tests::transpose_*`).
- **AK-3** Add Carets to Ends of Selected Lines: several lines, a selection ending at a line
  start, no selection, several carets, the last line without a line break, `\r\n`, the primary
  caret (`small::tests::carets_*`).
- **AK-4** Convert Indents: tabs and spaces mixed, both directions, a tab after text untouched,
  lines of white space only, a selection (also one starting inside an indentation, deviation
  IMP-0403), `\r\n`, the count of changed lines, carets kept on their text, tab widths 2 and 8
  (`small::tests::indents_*`).
- **AK-5** The six commands are in `COMMANDS`, run through `perform`, are one undo step each and
  leave a message where behaviours 3 and 12 name one
  (`commands::tests::listed_commands_are_handled_and_others_are_not`,
  `small::tests::commands_run_through_perform`).
- **AK-6** The rows of the copy popup for a file inside and outside the project folder
  (`copy_path_model::tests`).
- **AK-7** Declared effects (`security::tests::small_editing_commands_declare_their_effects`);
  titles, groups and the shortcut in Find Action
  (`everywhere::tests::small_editing_commands_have_titles`).
- **AK-8** `editor_action` with the nine new actions (`protocol.rs`:
  `editor_action_small_editing_commands`).
- **AK-9** In the window, without a screen: Sort Lines through the editor view, Convert Indents
  with the file's tab width, a read-only text unchanged with its message, the copy popup's rows
  and the clipboard after choosing one
  (`crates/workbench/tests/it/editor_actions_keymap.rs::small_editing_commands_in_the_window`).
- **AK-10** On screen next to the reference, and ⌥⇧G from a real keyboard: open (hand test).
- **AK-11** Qualified names (behaviour 13c): Java and Kotlin types, nested types, members,
  constructors, overloaded methods, members of anonymous and local classes, a file without a
  package, a Kotlin top-level function; Python classes, methods, nested classes, functions inside
  functions, `__init__`, a module below a source folder; Rust modules of the file, inline modules,
  `impl` blocks with generics and traits, a file outside `src`; names outside ASCII; the caret
  outside every symbol and on a local variable; a language without rules
  (`reference_name::tests`).
- **AK-12** The rows for an editor and for the tree's selection: the path with the line, the
  path from the innermost source folder, the file's reference name inside and outside a source
  folder and outside the project, one line per selected item with items lacking a path left out,
  a folder, a remote document with the host's paths and no source folder, the status text cut to
  its first line and 80 characters (`copy_path_model::tests`); the source folders of a project
  and the innermost one of a file (`source_roots::tests::source_folder_of_a_file`).
- **AK-13** Titles, group and shortcuts of the new actions in Find Action
  (`everywhere::tests::small_editing_commands_have_titles`); their declared effects
  (`security::tests::small_editing_commands_declare_their_effects`); the entries of the tree's and
  the tab's menu (`context_menu::tests::copy_entries_of_tree_and_tab`).
- **AK-14** `editor_action` with `copy_source_root_path`, `copy_path_with_line` and
  `copy_reference` on a file without naming rules, and the failure outside a source folder
  (`protocol.rs`: `editor_action_copy_reference_and_paths`).
- **AK-15** In the window, without a screen and without a language server: the rows of the
  popup for an editor, Path with Line Number and Copy Reference with the clipboard and the status
  bar, the popup and Path from Source Root for two files selected in the project tree
  (`crates/workbench/tests/it/editor_actions_keymap.rs::small_editing_commands_in_the_window`,
  `copy_paths_of_the_tree_selection`).
- **AK-16** A qualified name through a running language server in the window, the entries of the
  tab's menu carried out, ⌥⇧⌘C from a real keyboard, and the comparison with the reference:
  open (hand test).

## Out of scope
Fill Paragraph (behaviour 14). The link for the vendor's launcher among the entries of the open
copy popup. Resolving the reference under the caret to the symbol it points to, and names for
languages other than Java, Kotlin, Python and Rust: both need more than the document symbols of
the file. Pasting a copied reference as code (the open platform shortens and imports it). A limit
on the number of carets. Sorting with options (case, numbers, removing duplicates).

## Known deviations from the reference
- **Convert Indents measures from the start of the line (IMP-0403).** The open code starts
  measuring a line's indentation at the start of the selection when the selection begins inside
  it, and then replaces the whole indentation by the smaller width, so the first selected line
  loses columns. Ferris IDEA measures every line's indentation from its first column.
- **Sort Lines and Reverse Lines on fewer than two lines** answer in the status bar; the open
  action is disabled then (SPEC-0124: menu items are not greyed out).
- **Messages** of Convert Indents appear in the status bar, not in a hint at the caret, and in
  Ferris IDEA's wording.
- **Copy Path/Reference…** lists the rows in the order of the open group without its separators
  and without the launcher link.
- **Copy Reference names the symbol around the caret.** The open action names the element the
  caret is on — a declaration's name or, resolved, the target of a reference — and copies file
  and line anywhere else, also inside a method body. Ferris IDEA has only the file's document symbols
  and names the innermost symbol that holds the caret; a reference under the caret is not resolved.
- **Java names:** the parameter list of an overloaded method is the server's, with simple type
  names (`add(int, String)`); the open code writes qualified types (`add(int, java.lang.String)`).
  A member of an anonymous class is copied by its simple name; the open code names the type the
  anonymous class extends (`java.lang.Runnable#run`), which needs resolving. Packages, folders and
  Java files selected in the project tree copy paths only; the open action offers a package or
  class name for them.
- **Kotlin names:** the open code names the JVM view: a top-level function after the file's
  class (`shop.CartKt#total`), a property after its getter (`shop.Cart#getTotal`), an overloaded
  function with JVM parameter types. Ferris IDEA copies `shop.total`, `shop.Cart#total` and the
  server's parameter list, if its name has one.
- **Python modules** are named from the source folder or the project folder; the open code takes
  the shortest name the file can be imported under, which also knows the interpreter's paths.
- **JSON, YAML and properties files** have reference providers in the open sources (key paths);
  Ferris IDEA copies file and line for them. **Rust names** are Ferris IDEA's own form.
- **The Copy Reference row of the popup** shows no text for a language with naming rules; the
  open popup shows the name.
- **Clipboard:** the open action also puts the reference on the clipboard in a format of its own,
  so that a paste into code can shorten and import it. Ferris IDEA writes plain text only.
- **Status texts** are Ferris IDEA's own wording and cut long texts.
- **A file's reference name** uses a source folder for every file in one; the open rule does so
  in modules of the Java type only, which are the only ones Ferris IDEA has source folders for.
- **Remote documents** copy file and line for Copy Reference: their symbols are not asked of the
  host.
- **Further carets** of Sort Lines and Reverse Lines keep their offsets; how the open platform
  moves them was not derived from its sources.
- Nothing of this specification was compared with the reference on screen.
