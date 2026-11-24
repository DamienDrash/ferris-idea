# SPEC-0334: TODO tool window

- **Version:** 1 · **Date:** 2026-11-14
- **Sources:** OSS (`JetBrains/intellij-community` at commit
  `d4018fb57c367e7b5d895bd8a0c84c382efd0e17`, Apache-2.0, read as a logic template, no code
  taken over:
  `platform/editor-ui-ex/src/com/intellij/ide/todo/TodoDefaultPatternProvider.kt` and
  `TodoConfiguration.java` (the two default patterns, not case-sensitive, multi-line on by
  default);
  `platform/editor-ui-ex/src/com/intellij/psi/impl/search/IndexPatternSearcher.java` (patterns
  are looked for inside comment ranges, a plain-text file is one range, one occurrence per start
  offset, the continuation lines of an occurrence) and
  `platform/indexing-impl/src/com/intellij/psi/impl/search/IndexPatternBuilder.java` (a language
  may allow characters besides white space before a continuation);
  `platform/todo/resources/intellij.platform.todo.xml` (tool window `TODO` anchored at the
  bottom, the group of the three grouping actions);
  `platform/todo/src/com/intellij/ide/todo/TodoView.java` (tabs for the project and the current
  file, packages shown by default in the project tab), `TodoPanel.java` (toolbar: previous and
  next occurrence, filter, expand and collapse all, group by, preview; flatten needs packages),
  `SetTodoFilterAction.java` (a filter choice with "show all"),
  `model/TodoGrouping.kt` (modules, packages or folders, flattening, chains of empty packages
  shown as one node), `model/TodoScope.kt` (the scopes: project, current file, changelist, named
  scope), `nodes/TodoItemNode.java`, `TodoFileNode.java`, `TodoDirNode.java`,
  `SummaryNode.java` (a row is the line number and the line's text, files and groups carry their
  number of items, the top row the totals), `TodoFileDirAndModuleComparator.java` (names compared
  without regard to case);
  `platform/platform-impl/src/com/intellij/toolWindow/defaultToolWindowlayoutProvider.kt` (the
  default layout names no TODO window);
  the action ids `TodoViewGroupByShowModules`, `TodoViewGroupByShowPackages` and
  `TodoViewGroupByFlattenPackage` with their shortcuts in the ported keymaps, SPEC-0028,
  `docs/research/KEYMAP_COVERAGE.md`), GEN (a TODO comment is a note in a comment that work is
  left; bounded background work; everything not marked OSS), SPEC-0029 v2 and SPEC-0148 (the
  project's own files), SPEC-0027 (grammars and what they capture as a comment), SPEC-0061 and
  SPEC-0167 (tool windows), SPEC-0293 v6 behaviour 18 (keys that belong to the focused tool
  window), SPEC-0067 v5 behaviour 12 (source preview), SPEC-0275 (trees), SPEC-0317 (remote
  projects), SPEC-0319 (UI driver: declared effects), ADR-0008, ADR-0009. Nothing was observed in
  the reference installation.
- **Feature matrix rows:** none yet (`docs/research/PARITY.md` has no row for this feature)

## Purpose
See every TODO and FIXME note in the comments of the project's own files in one tool window:
as a tree by folder and file with counts, with a preview of the place, for the whole project or
for the file being edited, and jump to each. Agents read the same list through MCP.

## Behaviour

### What is found
1. **Files:** the project's own files as Find in Files takes them for the place "Project"
   without a file mask (SPEC-0029 v2 behaviours 7 and 10): no installed packages, environments,
   build output, version control data, IDE data, folders the project model excludes or files Git
   ignores; a file Git tracks is the project's own. Symbolic links are not followed, so nothing
   outside the project folder is read. Binary files and files over 10 MiB are left out as there
   (SPEC-0029), and a file over 8 MiB is left out because no syntax tree is built for it
   (SPEC-0027). [SPEC-0029, SPEC-0148, SPEC-0027]
2. **Patterns:** two patterns, each a regular expression that is not case-sensitive:
   `\btodo\b.*` (named `todo`) and `\bfixme\b.*` (named `fixme`). A match starts at the word and
   runs to the end of its line. [OSS]
3. **Only in comments:** in a file whose language has a grammar (SPEC-0027) a match counts only
   when it starts inside something the grammar captures as a comment, embedded languages
   included (a script block in HTML); a match is cut at the end of that comment. A plain-text
   file (`.txt`, `.text`) is searched as a whole, as if all of it were one comment. Any other
   file — a language without a grammar, an unknown type — is not searched, because Ferris IDEA
   cannot tell its comments from its code; when such a file contains a match, it is counted as
   "not checked" (behaviour 22) instead. [OSS: comments only, plain text as one range; GEN: the
   rest]
4. **An item** is one match: its file, its line and column (the place of the first character of
   the match), the name of its pattern, and its text. The text is the match without white space
   at its end; when the match ends where a block comment ends, the closing `*/` or `-->` and
   the white space before it are not part of the text. [OSS: the match; GEN: the cut]
5. **Several in a line:** every match with a start of its own is an item. A line with both
   words gives two items: the first from the first word to the end of the line, the second from
   the second word. Two patterns that match at the same place give one item, of the pattern that
   stands later in behaviour 2. [OSS]
6. **Continuation lines:** an item goes on over the following lines as long as each of them
   - has, at the column where the item's match starts, a space or a tab, followed after any
     further spaces and tabs by other text before the line ends (the text is indented deeper
     than the word);
   - has that text inside the same comment, or inside the comment that follows it (a run of
     line comments);
   - has the comment begin, in that line, no further right than the item's comment begins in
     its line, and has nothing but spaces, tabs and the characters `/`, `*`, `#`, `-`, `;`, `!`
     and `%` between the comment's begin in that line and the item's column;
   - contains no match of any pattern (such a line starts an item of its own).
   The first line that fails ends the item. The text of each continuation line, from its first
   character that is not a space or tab to the end of the line or of the comment, is added to
   the item's text after a single space. [OSS: the rule; GEN: one set of leading characters for
   all languages instead of one per language, and the joined text]
7. **Limits:** at most 5 000 items are kept for the project; further files are not read and the
   result says that it is cut. An item's text and a row's line text are cut after 300
   characters, with `…` at the end. An item has at most 20 continuation lines. [GEN]
8. **Order:** files by their path relative to the project (path parts compared as text), items
   of a file by line, then column. The order does not depend on the order in which files were
   read. [GEN]
9. **Unsaved text:** for a file open in an editor with unsaved changes the editor's text is
   searched instead of the file's content, in both scopes of the window. MCP reads the files
   (behaviour 27). [OSS: the open platform searches the document; GEN]

### Scanning
10. **In the background:** the project is scanned off the UI thread. A scan that is no longer
    wanted (the window was hidden, the project closed, a newer scan started) is told to stop and
    stops before the next file. While the first scan runs, the window says that it is scanning;
    a later scan replaces the shown result only when it is complete. [GEN]
11. **Only what changed:** a scan keeps, per file, its modification time and size with the
    items found; the next scan reads only files whose time or size changed, files that are new,
    and files with unsaved changes, and forgets files that are gone. [GEN]
12. **When:** the first scan starts when the window is shown for the first time. While the
    window is shown, a scan starts after a file was saved in an editor, and the files are
    looked at again every two seconds so that changes made outside (another program, a checkout,
    an agent) show without a key press. The current file's items follow its editor's text
    within half a second after an edit. While the window is hidden nothing is scanned. [GEN]

### Tool window
13. **"TODO" tool window** (`ActivateTODOToolWindow`; the keymaps give it no shortcut): a tool
    window of the layout with the shared header (SPEC-0061 v5 behaviour 18), docked at the
    bottom by default, reached through the More tool windows list until the user adds it to the
    sidebar (SPEC-0061 v10 behaviours 51–52), with the open platform's TODO tool window icon on
    its stripe button. The action shows and focuses it, and hides it when it has the focus, like
    the other tool windows. View › Tool Windows lists "TODO" after "Find"; this is the only
    menu entry, and SPEC-0124 is not changed by this specification. [OSS: id, title, bottom, not
    in the default layout, icon; SPEC-0061, SPEC-0167]
14. **Scopes:** the header has two tabs, "Project" and "Current File". Project shows the items
    of all files of behaviour 1. Current File shows the items of the file of the active editor,
    whatever file it is (also one that is not the project's own), and follows the active editor;
    without an editor, or for an editor without a file, it says that no file is open. [OSS: the
    two tabs; GEN]
15. **Tree:** the top row is the project (its folder's name) with the totals: the number of
    items and of files. Below it, depending on the grouping (behaviour 16): modules, folders,
    files; below a file one row per item. A file row shows the file's name and its number of
    items; a module or folder row its name and the number of items below it. An item row shows
    the line number (1-based) and the text of the item's first line without the white space at
    its ends, the match set off from the rest of the line. Folders stand before files; modules,
    folders and files are ordered by name without regard to case, then by name as written; items
    as in behaviour 8. All nodes are open at first. In Current File the tree is the file and its
    items. [OSS: totals, counts, line number and line text, order by name without case; GEN]
16. **Grouping** (Project scope), three toggles:
    - `TodoViewGroupByShowModules` (off by default): a row per module between the project and
      the rest. A module is the nearest folder above the file that holds a build file
      (SPEC-0293 v3 behaviour 14), named by that folder; files outside every such folder belong
      to the project's own module.
    - `TodoViewGroupByShowPackages` (on by default): the folders between the module (or the
      project) and the file are rows, nested as the folders are. A folder that holds no file
      with items of its own and exactly one folder with items is shown together with that
      folder as one row (`src/main/java`). Off: files stand directly below the module or the
      project, each with its folder dimmed after the name.
    - `TodoViewGroupByFlattenPackage` (off by default; has an effect and is enabled only while
      packages are shown): every folder that holds a file with items is one row directly below
      the module or the project, named by its whole path from there; folders are not nested.
    The toggles are buttons of the window's toolbar and are kept for the session. [OSS: the
    three toggles, their defaults and the dependency, chains as one node; GEN: folders stand for
    packages, modules by build file]
17. **Filter:** a choice of three in the toolbar: "All" (default), "todo", "fixme". With a
    pattern chosen only its items are shown and counted; files and folders without such items
    are not shown. The choice holds for both scopes and for the session. [OSS: a filter with a
    "show all" entry; GEN: the built-in choices]
18. **Keys and mouse in the tree:** Up and Down select; Left closes a node or goes to its
    parent, Right opens it; Enter, F4 or a double click on an item opens its file and puts the
    caret at the item's line and column, with the place left remembered for Back; on a file they
    open the file; on any other row they open or close it. A single click selects. The focus
    goes to the editor after a jump. [GEN, SPEC-0275]
19. **Next and previous occurrence** (`NextOccurence`, `PreviousOccurence`): while the TODO
    window has the focus, or is shown while the Find tool window shows no usages and the Debug
    window has no focus, they select the next or previous item in tree order, whether its
    parents are open or not, open them, and jump to it as Enter does; the focus stays in the
    window. At the last or first item they do nothing. [OSS: the window is an occurrence
    navigator; GEN: precedence]
20. **Preview:** a toggle of the toolbar (on by default) shows, beside the tree, the source
    around the selected item with the item's line marked and the syntax coloured (SPEC-0067 v5
    behaviour 12): six lines before and eight after. For a row that is no item, and for a file
    over 1 MiB, the preview is empty. [OSS: a preview toggle; GEN]
21. **Expand All and Collapse All:** two buttons of the toolbar open every node or close every
    node below the project. [OSS]
22. **What the window says:** while the first scan runs: that it is scanning. Without items:
    that no TODO items were found (with a filter: none for that pattern). Below the tree, when
    it applies: that the list is cut at 5 000 items; the number of files not read (binary, too
    large or unreadable); the number of files with a match but without a known comment syntax
    that were not checked (behaviour 3). [GEN]
23. **Remote projects:** not supported. In a window on a remote project (SPEC-0317) the TODO
    window says that TODO items are not available for remote projects and scans nothing: the
    scan reads local files, and the host has no function for it yet. [GEN]

### Actions, Find Action, UI driver
24. **Titles:** Find Action lists "TODO" (the tool window, group Tool Windows), "Group by
    Modules", "Group by Packages" and "Flatten Packages" (group TODO), each with its shortcut.
    [GEN]
25. **Keys of the toggles:** the shortcuts of the three grouping actions (⌃M, ⌃P and ⌃F in the
    macOS keymap, ⌃⌥M, ⌃⌥P and ⌃⌥C in the default keymap) act while the TODO window has the
    focus and not elsewhere, where the keys belong to other actions (SPEC-0293 v6 behaviour 18).
    Run by id (Find Action, a menu, MCP `run_action`) they flip the toggle of the TODO window
    when its view exists; otherwise the status line says that the TODO window is not open. In
    the Current File scope they change the grouping the Project scope will show. [OSS: ids and
    keys; SPEC-0293]
26. **Declared effects (SPEC-0319 behaviour 35):** `ActivateTODOToolWindow` and the three
    grouping actions are declared with no effect. The rows of the tree, the scope tabs and the
    toolbar buttons are annotated elements of the semantic tree. [SPEC-0319]

### MCP (ADR-0008)
27. **`todo_items`** (read-only; needs no right beyond reading, changes nothing, is not
    logged) takes
    - `scope`: `project` (default) or `file`;
    - `path`: with `scope: "file"`, the file, relative to the workspace; it must lie inside the
      workspace folder with no symbolic link on the way;
    - `pattern`: `todo` or `fixme`; without it both;
    - `limit`: the most items returned, 1 to 5 000, default 1 000.
    It returns `items` in the order of behaviour 8, each with `path` (relative to the
    workspace, `/` between parts), `line` and `column` (1-based; the column counts characters),
    `pattern` and `text` (behaviours 4, 6, 7); `truncated: true` when there were more than
    `limit` items or the scan was cut (behaviour 7); `files_not_read` and `files_not_checked`
    (behaviour 22). It reads the files as saved; text not yet saved in a window is not seen.
    Wrong arguments (an unknown scope or pattern, a limit outside its range, `scope: "file"`
    without `path`, a path outside the workspace or behind a link, a file that is not there) are
    refused with a message that names the argument. [GEN, ADR-0008]

## Acceptance criteria
- **AK-1** Given a Rust, a Java, a Python and an HTML file with the words in comments, in
  string literals and in identifiers, in any mix of upper and lower case, then exactly the
  matches in comments are items, with their line, column, pattern and text; `todos` and
  `mytodo` are no matches; a block comment's closing `*/` is not part of the text. (Behaviours
  2–5; unit tests)
- **AK-2** Given a `.txt` file and a file of an unknown type that both contain `TODO`, then the
  text file's line is an item and the other file gives no item and is counted as not checked.
  (Behaviour 3; unit test)
- **AK-3** Given a line comment `// TODO first` followed by `//      second` and `// third`,
  and a block comment with an indented second line, then the items' texts are `TODO first
  second` and the block's two lines joined, `third` belongs to no item, and a deeper-indented
  line that itself holds `FIXME` starts an item of its own. (Behaviour 6; unit test)
- **AK-4** Given a line with 1 000 characters after `TODO`, text before the word that is not
  ASCII (`ä`, `日本`), and a project with more items than the cap, then the text is cut at 300
  characters with `…`, the column counts characters, and the scan keeps exactly the cap and
  says that it is cut. (Behaviours 4, 7; unit tests)
- **AK-5** Given a project with a file in an ignored folder, a file in `node_modules`, a
  symbolic link to a file outside the project, a binary file and an own file, each with `TODO`
  in a comment, then only the own file has items; after the file's text changes on disk, the
  next scan shows the new items and reads only that file; a scan whose stop flag is set returns
  nothing. (Behaviours 1, 8, 10, 11; unit tests)
- **AK-6** Tree model: for files in two modules and nested folders, the rows, their depth and
  their counts for each combination of the three toggles (packages on by default, a chain of
  single folders as one row, flattened folders with their whole path, modules as a level,
  flatten without packages changes nothing); the filter by pattern drops files and folders
  without such items and recounts; closed nodes hide their rows; the totals of the top row.
  (Behaviours 15–17; unit tests)
- **AK-7** Tree model: next and previous step through the items in tree order through closed
  nodes and stop at the ends; the label of an item is its 1-based line and the trimmed line text
  with the range of the match. (Behaviours 15, 19; unit tests)
- **AK-8** MCP: `todo_items` is listed with its schema; for a workspace with TODO and FIXME
  comments it returns the items of the project in order with 1-based line and column, only one
  pattern with `pattern`, at most `limit` items with `truncated`, the items of one file with
  `scope: "file"`, and refuses the wrong arguments of behaviour 27. (Protocol test)
- **AK-9** In a window (test harness, no screen): `ActivateTODOToolWindow` shows the window at
  the bottom with the tabs Project and Current File and, after the scan, the tree of behaviour
  15; Enter on an item opens the file with the caret at the item; `NextOccurence` goes to the
  next item; the grouping actions change the tree while the window has the focus and their
  chords run nothing of the TODO window while the editor has it; after an edit in the editor
  and a save the tree shows the new item; Current File shows the active editor's items
  including unsaved text. (Behaviours 9, 12–19, 25; window tests)
- **AK-10** The tool window is registered: id `todo`, title `TODO`, action
  `ActivateTODOToolWindow`, bottom, off the sidebar by default, a bundled stripe icon, a row in
  View › Tool Windows; Find Action knows the four titles; the UI driver knows the four actions
  as without effect; the three grouping actions belong to the TODO window's focus. (Behaviours
  13, 24–26; unit tests)

## Out of scope
- Custom patterns, their colours and icons, and named filters over them in the settings: the
  two default patterns and the three filter choices are built in. The reference's settings page
  for TODO was not looked at.
- Highlighting TODO comments in the editor in a colour of their own and showing them on the
  error stripe.
- The scope-based tab, the tab for the files of a changelist, and the check before a commit
  that looks for new TODO items.
- Auto-scroll to source as a toggle; the preview (behaviour 20) shows the place without opening
  it.
- Keeping the grouping, the filter, the scope and the open nodes across restarts.
- TODO items in a remote project (behaviour 23) and in files that are not the project's own
  (libraries, ignored files), except the current file (behaviour 14).
- Languages without a grammar (behaviour 3): no comment syntax is guessed.
- A shortcut for the window: the keymaps bind none; the user can assign one in the keymap
  settings (SPEC-0105).
- Editing or removing an item from the window, and items as tasks with a state.

## Known deviations from the reference
- Nothing was observed in the reference; the behaviour follows the open platform's sources
  named above and decides the rest (GEN). A later observation may change these behaviours.
- **Comments come from Tree-sitter grammars**, not from the open platform's lexers: what counts
  as a comment is what the grammar's highlighting captures as one. A file type without a
  grammar is not searched and is counted as not checked; the open platform searches every file
  type it has a lexer for.
- **One set of leading characters for continuation lines** (`/`, `*`, `#`, `-`, `;`, `!`, `%`)
  instead of a set per language (behaviour 6), and at most 20 continuation lines.
- **An item is one row**: continuation lines are joined to the text with a space instead of
  being shown as further lines of the row.
- **The closing `*/` or `-->` of a block comment is not part of an item's text** (behaviour 4).
- **Packages are folders**: the tree groups by the folders below the module or project, also
  for Java and Kotlin sources, where the open platform shows package names below source roots.
  Modules are the folders with a build file, as in the Find tool window (SPEC-0293).
- **Bounded**: 5 000 items, 300 characters per text, files up to 8 MiB (behaviours 1 and 7); the
  window and the MCP tool say when something was left out.
- **Scanned on demand**, without an index kept on disk: only while the window is shown, with
  the unchanged files taken from the last scan (behaviours 11, 12).
- **Agents read the list** through the MCP tool `todo_items` (ADR-0008).
