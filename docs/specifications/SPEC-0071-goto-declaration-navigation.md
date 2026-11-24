# SPEC-0071: Go to Declaration and Back/Forward

- **Version:** 13 · **Date:** 2026-11-18 (v13: a file opened from the Project tree or from Recent Files is a jump for Back and Forward, behaviour 4a, AK-15; window check A4 of PARITY row F-017, gap N1); 2026-11-14 (v12: Header/Source, behaviour 19, AK-14; ninth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v11: the 30 s limit holds for the server's answer too,
  behaviour 1; v10: Go to Test and Go to Related, behaviours 15–18, AK-11 to AK-13; docs/research/KEYMAP_COVERAGE.md; v9: previous and next method, last and next edit location of the file, behaviours 12–14; v8: in an HTML file a class, an id and a jump link have a declaration, OBS-0039 W1 and W2; v2: works on unsaved text; v3: several definitions in a chooser, SPEC-0081; v4: shortcuts on keyboard layouts without `[`, hand test; v5: a caret right after a name, messages naming the reason, hand test round 5 Nr. 12 and 16; v6: one place named twice is one target, a target that cannot be opened says so, hand test Nr. 110, D61; v7: on a declaration ⌘B lists its usages, a jump opens the fold it lands in, OBS-0016)
- **Sources:** GEN (jump from a use to its declaration by shortcut or modifier-click, and back and forward
  through earlier places; F-034, F-020), OSS (action ids `GotoDeclaration` with ⌘B and the mouse shortcut
  ⌘-click, `Back` ⌘[ / ⌘⌥←, `Forward` ⌘] / ⌘⌥→ of the ported keymaps), SPEC-0024 (definitions from the
  language server), SPEC-0058 (editor view)
- **Feature matrix rows:** F-034, F-020

## Purpose
Follow code to where it is defined and return to where you were.

## Behaviour
1. **Go to Declaration:** `GotoDeclaration` (⌘B on macOS, Ctrl+B elsewhere) or ⌘-click (Ctrl-click
   elsewhere) on a symbol asks the file's language server for its definition in the background (30 s
   limit; the status bar says it is looking). v11: the limit holds for opening the file and for the
   server's answer each; until v10 the answer had up to 60 s (SPEC-0042 v21 behaviour 37). The server receives the editor's current text, so unsaved
   edits count (SPEC-0072 behaviour 6). [GEN, OSS]
2. **Result:** a single definition that is not on the caret's own line opens in a tab with the caret at
   its start; several are offered in a chooser popup (SPEC-0081 behaviour 2). Without a result the status
   bar says so; without a server it says which server is missing and how to install it (SPEC-0042 v7
   behaviour 15), and when the server failed it gives its reason. A declaration only in a library
   class says so (SPEC-0042 v7 behaviour 16). [GEN]
3. A ⌘-click places the caret at the click before asking, so the symbol under the pointer is used. [GEN]
4. **Back and Forward:** jumps — Go to Declaration, Go to Line, opening a symbol from Search Everywhere,
   opening a match from Find in Files — record the place they leave (file, line, column; one place per
   line). `Back` returns to the last recorded place and makes the current one available to `Forward`;
   a new jump clears the Forward places. At most 100 places are kept; places in deleted files are
   dropped when reached. [GEN, OSS]
4a. **Opening a file is a jump (v13):** opening a file from the Project tool window — a double
   click on its row, Enter or Jump to Source (⌘↓, F4) with its row selected — and from the Recent
   Files popup (SPEC-0069 behaviour 4) records the caret's place in the active editor like the
   jumps of behaviour 4, when the file opened is another file than the active one. Back then
   returns to that file and its caret place, and Forward to the opened file at the place its caret
   had when Back was used. Opening the active file again, or a file while no editor is active,
   records nothing. Until v12 these openings were no jumps, so Back stayed in the opened file.
   [OBS: window check of PARITY row F-017, 2026-11-18, Project tree only; GEN for Recent Files]
5. The MCP counterpart is `find_symbol_locations` (SPEC-0024). [ADR-0008]
6. **Keyboard layouts:** a shortcut whose key the current layout cannot type without ⌥ (⌘[ and ⌘] for
   Back and Forward) also works on the key in the same place, as other macOS applications move it
   (German: ⌘ö and ⌘ä); this applies to every action of the keymap when the pressed keystroke has no
   action of its own. ⌘⌥← and ⌘⌥→ keep working. [GEN, IMP-0031]
7. **Caret right after a name (v5):** when the caret stands directly after a name (letters, digits,
   `_`, `$`) and not on one — `person|.greet()`, `Person|` at the end of a line — Go to Declaration,
   Type Declaration and Implementation ask about that name, as a caret inside it would. A caret
   after a `;`, a bracket or a space at the end of a line still finds nothing. [GEN]
8. **One place, one target (v6, D61):** places the server reports more than once — ruby-lsp names
   a file it indexed twice as `root//greeter.rb` and `root/greeter.rb` — count once: file
   addresses are read with repeated separators and `.` components dropped, and equal places are
   merged, so a single declaration is a direct jump and not a chooser of identical rows. A target
   whose file does not exist (any more) is not a silent no-op: the status bar says "Cannot open
   <path>: the file does not exist." [GEN, STD]
9. **On a declaration (v7):** when every definition the server reports lies on the caret's own line —
   the caret stands on the declaration itself — Go to Declaration (⌘B, ⌘-click) shows the usages
   popup of SPEC-0073 for that symbol instead of saying that nothing was found, as the reference
   does (OBS-0016 Nr. 2). Since SPEC-0073 v5 the list leaves the declaration out, and a single
   usage is a hint (or a jump) instead of a popup. [OBS-0016]
10. **Folds (v7):** a jump — Go to Declaration, Implementation, Type Declaration, a usage, Back and
   Forward — that lands on the first line of a folded region (a method folded to one line) or inside
   one unfolds that region, so the target is shown with its body. [OBS-0016]
11. **HTML: classes, ids and jump links (v8):** in an HTML file a word of a `class` attribute and
    the value of an `id` attribute lead to the rules that define them in the stylesheets, and the
    fragment of an `href` leads to the element with that id. Ferris IDEA answers these places itself,
    without a language server; where, in which order and within which bounds is SPEC-0042 v17
    behaviours 28, 29, 32 and 33. One target opens directly, several come in the chooser
    (behaviour 2), a name without a rule gives the message for no declaration. Behaviour 7 holds:
    a caret directly after the word asks about it. [OBS-0039, SPEC-0042]
12. **Previous and next method (v9; `MethodUp` ⌃⇧↑ / Alt+Up, `MethodDown` ⌃⇧↓ / Alt+Down):** the
    caret goes to the name of the nearest method or function declaration before (`MethodUp`) or after
    (`MethodDown`) it in the file: the declarations are the definitions of kind method or function the
    file's syntax tree gives (SPEC-0031 behaviour 5), in document order. Selections and further
    carets are dropped, a fold hiding the target opens (SPEC-0083 behaviour 4). Before the first or
    after the last declaration, in plain text, and in a language without definitions the caret stays.
    The place left is not recorded for Back (behaviour 4 lists the jumps that are). The tree comes
    from the caller as in SPEC-0057 behaviour 13, so `editor_keystrokes` carries the actions out too.
    [OSS (action ids), GEN, ADR-0008]
13. **Last and next edit location (v9; `JumpToLastChange` ⇧⌘⌫ / Ctrl+Shift+Backspace,
    `JumpToNextChange`, no shortcut):** the places are those of the undo steps of the file's history,
    newest first: for each step the end of the text its last edit put in (where a deletion was made,
    for a step that only removed text), moved along with the edits made after that step; a place on
    the same line as the place before it in this list is left out. The list holds at most the 100
    newest places, taken from the 1 000 newest undo steps. The first `JumpToLastChange` puts
    the caret (one caret, no selection) at the newest place, every further one at the next older
    place, and at the oldest it stays. `JumpToNextChange` goes one place towards the newest and does
    nothing before any `JumpToLastChange` or at the newest place. An edit, an undo or a redo starts
    the walk over; moving the caret does not. Steps that were undone are no places; a file without
    edits since it was opened has none, and the caret stays. [OSS (action ids), GEN]
14. **Find Action (v9):** the four actions are listed under Navigate as Previous Method, Next Method,
    Last Edit Location and Next Edit Location (SPEC-0031); `JumpToNextChange` is offered although no
    key is bound to it. [GEN]
15. **Test and test subject (v10; `GotoTest`):** from a source file the action goes to its test, from a test file to the file it tests, by file names alone and among the project's own files (SPEC-0148 v3 behaviour 7: no installed packages, build output or ignored files). A file is a test by the names the run markers use (SPEC-0045): `test_x.py` and `x_test.py`, `x_test.go`, `x.test.*` and `x.spec.*` with a JavaScript or TypeScript extension; for Java, Kotlin, Groovy, Scala, C#, PHP and Swift files, where the markers go by annotations, the names `XTest` and `XTests`. The counterpart of the test `XTest` is a file named `X`, the tests of `X` are the files with those names; Python and Go keep the extension, JavaScript and TypeScript files match across their extensions (`cart.ts` and `cart.test.tsx`), the JVM languages across theirs, the others keep the extension. Candidates are ordered by how many folder names they share with the file counted from the file's folder upwards (so `src/main/java/shop/Cart.java` prefers `src/test/java/shop/CartTest.java`), then by path. One candidate opens in a tab and the place left is recorded for Back; several are offered in the chooser of behaviour 3 with the title `Choose test for Cart.java (2)` or `Choose test subject for CartTest.java (2)`; none says so in the status line (`No test found for Cart.java`, `No test subject found for CartTest.java`) — no test is created and no dialog asks. A file of another language (Rust keeps its tests in the file itself or under `tests/` without a naming rule) says that Go to Test knows no naming rule for it. The contents of the files are not read: a test that tests a class under another name is not found. [OSS: id of the bundled keymaps; GEN: the naming conventions of the test runners; SPEC-0045]
16. **The key of Go to Test (v10):** the bundled macOS keymap binds `GotoTest` to ⇧⌘T, the chord Ferris IDEA's keymap gives to `ReopenClosedTab` as a named deviation (SPEC-0028 behaviour 7, SPEC-0059 v9.1 behaviour 22b). No shortcut changes: on a chord that also carries `ReopenClosedTab`, `GotoTest` does not run and the menu and Find Action show no key for it. It is reached through Navigate › Test and Find Action, and by a key the user gives it in Settings › Keymap. In the default keymap, where Reopen Closed Tab has another chord, Ctrl+Shift+T runs it. [SPEC-0028, SPEC-0059]
17. **Related files (v10; `GotoRelated`, ⌃⌘↑):** goes to the files Ferris IDEA's own model relates to the active file. The model has one relation: an HTML file and the stylesheets it links with `<link rel="stylesheet">` that are files of the project (SPEC-0042 v17), in both directions — from the HTML file to those stylesheets, from a CSS, SCSS or Less file to the project's HTML files that link it (at most 500 HTML files are read, the bound of SPEC-0042 behaviour 32). One related file opens, several are offered in the chooser (`Choose related file of index.html (2)`), none says so in the status line; for a file of another kind the status line says that no related files are known for it. Not related here: a Spring controller and its endpoints (they are places in the controller's own file, which File Structure and the Endpoints tool window list), a file and its test (behaviour 15), a class and its super types (hierarchies, SPEC-0082). [OSS: id and shortcut of the bundled keymaps; SPEC-0042 v17]
18. **Find Action, menu and MCP (v10):** Navigate gets Related Symbol… (`GotoRelated`) and Test (`GotoTest`) after Super Method (SPEC-0124 v5); Find Action lists them with these titles. MCP: `goto_test` (path) returns `known` (whether the file's language has a naming rule), `is_test`, `subject` (the name without the test affix) and `candidates` (workspace-relative paths in the order of behaviour 15); `related_files` (path) returns `known` and `files`, each with `path` and `relation` (`stylesheet` or `linked_from`). Both read only. [ADR-0008]
19. **Header/Source (v12; `SwitchHeaderSource`, F10 in the bundled keymaps; Navigate ›
    Header/Source after Test):** in a C, C++ or Objective-C file the action goes from a header
    to its source file and from a source file to its header, by file names among the project's
    own files (as behaviour 15): headers are `.h`, `.hh`, `.hpp`, `.hxx`, `.h++` and `.inl`,
    sources `.c`, `.cc`, `.cpp`, `.cxx`, `.c++`, `.m` and `.mm`; the counterpart has the same
    name without the extension, compared exactly. Candidates are ordered by the folder names
    they share with the file counted from its folder upwards (`src/shop/cart.cpp` prefers
    `include/shop/cart.h` to `include/cart.h`), then by path. One candidate opens in a tab and
    the place left is recorded for Back; several are offered in the chooser of behaviour 3
    titled `Choose header for cart.cpp (2)` or `Choose source for cart.h (2)`; none says so in
    the status line (`No header found for cart.cpp`). A file of another language says that
    Header/Source applies to C, C++ and Objective-C files. The contents are not read and no
    language server is asked. Find Action names it "Header/Source". MCP: `header_source` (path)
    returns `known`, `is_header` and `candidates` (workspace-relative, in this order); it reads
    only. [OSS: id and key of the bundled keymaps; GEN: the extensions of C and C++ toolchains]

## Acceptance criteria
- **AK-1** Navigation history: duplicates on one line, back, forward, a new jump clearing forward,
  forgetting a file (unit test).
- **AK-2** In a real window (`ferris-idea ui --selftest`, with `clangd`): from a call of `helper`,
  ⌘B moves the caret to its definition at 1:5; ⌘[ returns to 2:28.
- **AK-3** Use by hand: ⌘-click, several definitions, Forward.
- **AK-4 (v5)** The column asked for: inside a name, right after a name, after `;` and at the start
  of a line (unit test `nav_model::tests::the_name_before_the_caret_counts`).
- **AK-5 (v6)** With ruby-lsp, ⌘B on `Greeter` of `Greeter.new` finds the class once (live test
  `tests/ruby.rs`, both servers); doubled addresses and equal places count once (unit tests
  `insight::tests::parses_hover_and_locations`, `nav_model::tests::a_place_named_twice_is_one_target`).
- **AK-6 (v7)** ⌘B on a declaration asks for its usages; a jump to a folded one-line method
  unfolds it (unit tests `nav_model::tests::only_the_caret_line_means_a_declaration`,
  `fold_model::tests::a_jump_opens_the_fold_it_lands_in`); side by side in OBS-0016.
- **AK-7 (v8, behaviour 11)** SPEC-0042 AK-29, AK-30 and AK-33: the declaration request of the
  editor (`insight::found_in`) on unsaved text of `index.html` gives the rule `.card` of
  `css/site.css` for `class="card"` and `id="prices"` for `href="#prices"`, with no HTML
  server to ask (`crates/languages/tests/it/html_css.rs::html_answers_without_a_server`).
- **AK-8 (v9, behaviour 12)** From inside a body up to its method's name and on to the previous
  one, down to the next, the edges, no declarations (unit test
  `commands::tests::method_up_and_down`); the offsets of a Java class's methods and of Python
  functions, none for fields and classes (unit test `syntax::tests::method_offsets`).
- **AK-9 (v9, behaviour 13)** Places of edits on three lines with a later insertion above them,
  two edits on one line counting once, a deletion, an undone step (unit test
  `buffer::tests::change_places_follow_later_edits`); the walk back and forward, its edges and
  its restart after an edit (unit test `commands::tests::jump_to_last_and_next_change`).
- **AK-10 (v9)** MCP: `editor_keystrokes` with `control shift DOWN` in the macOS keymap on a Java
  file moves to the next method's name (protocol test `editor_actions_from_the_keymap`); in a
  window the four actions move the caret
  (`crates/workbench/tests/it/editor_actions_keymap.rs::methods_and_edit_locations`).
- **AK-11 (v10, behaviour 15)** Test names and subjects per language, the counterpart across the JavaScript and the JVM extensions and not across others, the order by shared folders, a file without a rule (unit tests `related::tests::test_names_and_their_subjects`, `related::tests::counterparts_in_both_directions_ordered_by_shared_folders`); titles and messages (`keymap_actions::tests::goto_test_titles_and_messages`). In the window: open.
- **AK-12 (v10, behaviour 16)** In the macOS keymap ⇧⌘T leaves `GotoTest` out and keeps `ReopenClosedTab`, and `GotoTest` shows no key; in the default keymap it keeps Ctrl+Shift+T (unit test `keymap::tests::goto_test_yields_the_chord_of_reopen_closed_tab`).
- **AK-13 (v10, behaviours 17 and 18)** The stylesheets of an HTML file and the HTML files of a stylesheet, links outside the project left out (unit tests `html_css::tests::linked_stylesheets_and_the_pages_that_link_one`, `related::tests::related_files_of_html_and_stylesheets`); the menu places (`menus::tests::navigation_actions_from_the_keymap_are_in_their_menus`) and titles (`everywhere::tests::navigation_actions_from_the_keymap_have_titles`); MCP `goto_test` and `related_files` (protocol test `navigation_tools_from_the_keymap_coverage`). In the window: open.
- **AK-14 (v12, behaviour 19)** Headers and sources in both directions, the order by shared
  folders, other languages unknown (unit test
  `related::tests::headers_and_sources_by_name_ordered_by_shared_folders`); titles and messages
  (`keymap_actions::tests::header_source_titles_and_messages`); in a window of the test harness
  F10 opens the one header and lists two sources (`ninth_package_keymap.rs::header_source_*`);
  MCP `header_source` (protocol test `header_source_tool`).
- **AK-15 (v13, behaviour 4a)** Opening another file records the place left, opening the active
  file or opening without an active editor does not, and Back and Forward walk between the two
  files (unit test `nav_model::tests::opening_another_file_is_a_jump`); in a window of the test
  harness a double click on a file row in the Project tree and then Back return to the first
  file at its caret, Forward to the opened file, and a file opened from Recent Files is a jump too
  (`tabs_and_tree_handtests.rs::back_returns_from_a_file_opened_in_the_tree`).

## Out of scope
Find Usages (SPEC-0073), Go to Implementation and Type Declaration (SPEC-0081),
recent locations, navigation within unsaved text, hover underline of links while ⌘ is held.
Edit locations across files (behaviour 13 walks the active file's history only). Highlight Usages in
File (`HighlightUsagesInFile`): the editor computes no highlight of the occurrences of the symbol at
the caret that the action could keep.

## Known deviations from the reference
The reference resolves declarations with its own index; Ferris IDEA asks the language server per
request. Compared side by side in OBS-0016 (2026-11-05, Java).

v8: classes, ids and jump links of HTML files are resolved from Ferris IDEA's own syntax trees; what
stays different there (classes built by scripts or templates, the choice among several rules) is
listed in SPEC-0042 v17. The jump itself was not repeated in a window next to the reference.

v13: opening from the Project tree was compared in the reference (Main.java, then Inventory.java by
a double click in the tree: Back returns to Main.java at its caret, Forward to Inventory.java at its
caret). Recent Files was not compared; other ways of opening a file — a click on an editor tab, the
Switcher, opening in the right split — record nothing.

v9: not compared with the reference. Known: the reference's Last Edit Location walks the edit
places of all files of the project; Ferris IDEA's walks those of the active file. Open there: whether
its Previous/Next Method also stops at classes and fields, and where on the declaration it puts
the caret.
