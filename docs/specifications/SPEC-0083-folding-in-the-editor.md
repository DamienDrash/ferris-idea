# SPEC-0083: Code folding in the editor

- **Version:** 10 · **Date:** 2026-11-15 (v10: the default of behaviours 7 and 8 checked against the
  open platform after OBS-0056; the name-and-parameters and right-margin conditions, behaviour 7a; v9: Custom Folding… (`GotoCustomRegion`), a list of the
  file's custom regions that puts the caret on the chosen one, behaviour 24, AK-30; seventh
  package of docs/research/KEYMAP_COVERAGE.md; v8: Collapse All leaves the outermost container open,
  custom regions between comment markers, folding the documentation comments, placeholders that
  show what is folded, folding arrows on hover with a setting, every action in Code › Folding and
  in `editor_action`, behaviours 18–23, AK-20 to AK-29, OBS-0044 FO1–FO5; v7: `CollapseBlock` and
  `CollapseSelection`, behaviours 15–17, AK-16 to AK-19; v6: expand and collapse recursively, expand to a level,
  behaviours 11–14; v5: a replacement that only puts lines in counts as whole lines put in, OBS-0027 L31; v2: one-line methods folded on open, OBS-0011; v3: the
  folded statement keeps its colours, only the braces are on the folded background, OBS-0013; v4: the
  import block of a Java or Kotlin file folded on open, folds beside inserted lines stay, OBS-0027 L19,
  L31)
- **Sources:** GEN (collapse and expand blocks with marks in the gutter, a placeholder for the hidden
  text, shortcuts; F-014), OSS (action ids `CollapseRegion` ⌘- / Ctrl+-, `ExpandRegion` ⌘= ⌘+ / Ctrl+=,
  `CollapseAllRegions` ⇧⌘- and `ExpandAllRegions` ⇧⌘= in the ported keymaps; scheme key
  `FOLDED_TEXT_ATTRIBUTES`), SPEC-0030 (folding regions from the syntax tree), SPEC-0058 (editor view),
  OBS-0011 step 2, finding 3 (v2: in an opened Java file the reference shows one-line method bodies
  folded next to the signature); v3: OBS-0013 item 3 (the reference keeps the colours of the folded
  code and puts only the braces on a grey background; IDECraft drew the whole placeholder grey);
  v6: OSS (action ids `ExpandRegionRecursively` ⌥⌘= / Ctrl+Alt+=, `CollapseRegionRecursively` ⌥⌘- /
  Ctrl+Alt+-, `ExpandAllToLevel1`…`5` and `ExpandToLevel1`…`5` with their two-key shortcuts in the
  ported keymaps, SPEC-0028), GEN (what expanding and collapsing with everything inside and down to a
  nesting level mean). No source text of the reference's actions was read and nothing was observed
  for v6; v7: OSS — the open platform's actions, `JetBrains/intellij-community` at the pinned
  commit `d4018fb` of APACHE_PORTING.md (PROVENANCE.md: logic template, no code):
  `platform/foldings/src/com/intellij/codeInsight/folding/impl/actions/CollapseBlockAction.java`
  (only languages that give a handler), `platform/foldings/src/com/intellij/codeInsight/folding/impl/CollapseBlockHandlerImpl.java`
  (the walk outwards from the block at the caret, what is passed over, when it stops) and
  `java/java-impl/src/com/intellij/codeInsight/folding/impl/JavaCollapseBlockHandler.java` (the
  blocks of Java: code blocks, the closing brace before the caret),
  `platform/foldings/src/com/intellij/codeInsight/folding/impl/actions/CollapseSelectionAction.java`
  and `CollapseSelectionHandler.java` beside it (a region from the selection, the same region
  removed, the region at the caret without a selection, a selection that partly overlaps a
  region), `platform/platform-impl/src/com/intellij/codeInsight/folding/impl/FoldingUtil.java`
  (which region counts as the one at the caret); the action ids, their texts
  (`platform/platform-resources-en/src/messages/ActionsBundle.properties`) and their shortcuts
  in the ported keymaps (⇧⌘. / Ctrl+Shift+. and ⌘. / Ctrl+.). SPEC-0319 (declared effects),
  ADR-0008. Nothing of v7 was observed in the reference IDE.
  v8: OBS-0044 (side by side on one Java file: what is folded after opening, what Collapse All
  folds and what it leaves open, the placeholders of imports, comments, a documentation comment
  and blocks, a custom region that shows its name, gutter marks that appear under the pointer,
  what stays folded at "all to level 1" and "all to level 2"; gaps FO1–FO5), GEN (comment markers
  for custom regions are a convention shared by editors: `region` / `endregion` and
  `<editor-fold desc="…">` / `</editor-fold>`; the first sentence of a documentation comment; a
  setting for marks that are shown all the time), SPEC-0030 v3 (the regions and their
  placeholders), SPEC-0129 (settings pages), SPEC-0303 (settings schema), ADR-0008. The action
  ids `ExpandDocComments` and `CollapseDocComments` are the ones v7 named as out of scope; no
  keymap binds them. No source text of the reference was read for v8.
- **Feature matrix rows:** F-014

## Purpose
Hide the parts of a file you are not working on.

## Behaviour
1. **Rows:** a folded region keeps its first line visible and hides the others; hidden lines take no
   rows, so scrolling, the line numbers, change markers and the scroll limit follow the visible rows.
   [GEN]
2. **Gutter and placeholder:** a column at the right of the gutter shows ▾ on the first line of every
   foldable region (from the syntax tree, SPEC-0030; none for plain text) and ▸ on folded ones. A
   click on ▾ folds the region, a click on ▸ unfolds it. A folded line ends with a `…` placeholder in
   the `FOLDED_TEXT_ATTRIBUTES` colours; a click on it unfolds. [GEN, OSS]
   **(v8)** When the mark of an open region is shown is behaviour 22; what the placeholder says is
   behaviour 21.
3. **Actions:** `CollapseRegion` folds the innermost open region around the caret (the caret moves to
   the end of its first line when it would be hidden), `ExpandRegion` unfolds the fold on the caret's
   line or around it, `CollapseAllRegions` folds every region (inner ones are absorbed), and
   `ExpandAllRegions` unfolds everything. [GEN, OSS]
   **(v8)** `CollapseAllRegions` leaves the outermost container of the file open (behaviour 18).
4. **Reveal:** a caret or a jump that lands on a hidden line (arrow keys, Go to Line, search, Go to
   Declaration) unfolds the regions hiding it. [GEN]
5. **Edits:** folds below an edit move with it; a fold the edit touches is unfolded. [GEN]
5a. **Whole lines put in (v4):** an insertion that only adds whole lines — text starting with a line break
   at the end of a line, or text ending with a line break at the start of a line — changes no existing
   line. Folds above the new lines stay folded, also one whose last line is the line before them; folds
   from the first moved line on move down; only a fold the new lines land inside is unfolded. So a member
   that Implement Methods, Override Methods or Generate appends after a method folded to one line leaves
   that method folded. [OBS-0027 L31, GEN]
   **(v5)** A replacement counts the same when the text it removes comes back unchanged around the
   text it adds: removed and inserted text are compared from the start and from the end, and when
   everything removed is found again there, what is left is the insertion that decides. jdtls adds a
   method by replacing the whitespace before the class's closing brace with the method and that
   whitespace again; before v5 this touched the line of the method above and unfolded it (OBS-0027
   L31 after #513). An edit that removes anything else still unfolds the folds it touches
   (behaviour 5). [OBS-0027 L31, GEN]
6. **Agents:** `folding_ranges` (SPEC-0030) lists the regions; folding is a view state without its
   own tool. [ADR-0008]
7. **One-line methods (v2, look v3):** when a Java file is opened, every method or constructor whose
   body holds exactly one statement on one line — the opening brace ending the line before it, after the
   signature, and the closing brace alone on the line after it — is folded. The signature line stays
   and its placeholder is the statement and the closing brace, so the method reads
   `int total() { return a + b; }`. The placeholder is drawn right after the brace. Since v3 the
   statement keeps the colours it has in the hidden line (syntax and semantic colours; the plain text
   colour while the syntax tree is being rebuilt), and only the two braces — the visible opening one
   and the closing one of the placeholder — get the `FOLDED_TEXT_ATTRIBUTES` background (the closing
   one also its foreground); nothing else of the line is greyed. A click on the placeholder, a caret moving in, or
   ⌘= unfolds it as any fold (behaviours 2–4). Only opening the file folds; edits never fold again.
   [OBS-0011, OBS-0013, GEN]
7a. **Default checked (v10):** the open platform folds one-line methods on open by default
   (`JavaCodeFoldingSettingsBase`: `COLLAPSE_ONE_LINE_METHODS = true`; `JavaBackendFoldings`: one
   statement on one line, the braces on the lines next to it, name and parameter list on one line,
   and the folded line within the right margin). IDECraft keeps the default and, since v10, also the
   two conditions it lacked: the method's name and the end of its parameter list stand on one line,
   and the folded line — the brace line up to `{` and the placeholder — is no wider than the right
   margin of Code Style (SPEC-0150, default 120). A method such as `int totalCents() {` /
   `return qty * unitPriceCents;` / `}` is still folded on open.
   [OSS, `JetBrains/intellij-community` at `d4018fb`, read only for the default and the
   conditions] OBS-0056 (Nebenbefund) saw the reference show `Inventory.java` of `cmp-java-ij`
   unfolded, while OBS-0011 saw it fold such methods. The project's `.idea` holds no folding
   option and no remembered folding of the file; the difference is taken to come from the
   reference's own state (its folding setting or the folding it remembers for the file), not from
   its defaults. Behaviour unchanged; whether the reference has its setting switched on is still an open question.
8. **Setting (v2):** Editor › General › Code Folding › *One-line methods* (on by default); also
   `fold_one_line_methods` in `set_ide_settings`. `folding_ranges` (SPEC-0030) marks these regions with
   `collapsed_text`, the placeholder. [GEN, ADR-0008]
9. **Imports (v4):** when a Java or Kotlin file with more than one import is opened, its import block is
   folded: from the line of the first import to the line of the last, with the blank lines and comments
   between the groups. The first import's line stays visible and ends with the `…` placeholder of
   behaviour 2; a click on it, ⌘=, a caret or a jump into the block open it as any fold (behaviours 2–4).
   A file with one import, or with all imports on one line, has nothing to fold. The block stays open
   when the file is opened at one of its lines (a jump to an import) or the caret is inside it. Only
   opening the file folds; edits never fold again, and an edit inside the block — Optimize Imports, an
   added import — opens it (behaviour 5). [OBS-0027 item 4 (L19), GEN]
10. **Setting (v4):** Editor › General › Code Folding › *Fold the imports of Java and Kotlin files when a
   file opens* (on by default); also `fold_imports` in `set_ide_settings` and under `[editor]` in the
   settings file. [GEN, ADR-0008]
11. **Recursively (v6):** `CollapseRegionRecursively` folds the innermost open region around the caret,
    as `CollapseRegion` does (behaviour 3, the caret moves the same way), and keeps every region inside
    it folded as well: when the outer fold is opened later — by `ExpandRegion`, a click, a caret or a
    jump — the regions inside it are folded, the outermost of them first (behaviour 13).
    `ExpandRegionRecursively` unfolds the fold on the caret's line, or else the innermost region around
    the caret, together with every fold inside it; afterwards nothing in that region is folded or
    kept as folded. Without a region around the caret both do nothing. [GEN, OSS]
12. **To a level (v6):** the level of a region is one more than the number of foldable regions that
    contain it, so a region inside no other has level 1. `ExpandAllToLevelN` (N from 1 to 5) leaves
    every region of the file down to level N open and folds the regions of level N + 1 with everything
    inside them (as behaviour 11 folds). `ExpandToLevelN` does the same inside the innermost region
    around the caret, which counts as level 1 there: that region and the regions inside it down to
    level N are open, the regions of level N + 1 inside it are folded with everything inside them;
    folds outside that region stay as they are, and without a region around the caret nothing happens.
    A caret that would be hidden moves to the end of the first line of the fold hiding it. [GEN, OSS]
    **(v8)** For `ExpandAllToLevelN` the outermost container of the file is no region and no
    level (behaviour 18): in a Java class the methods are level 1, the blocks directly in them
    level 2. `ExpandToLevelN` counts as before from the region at the caret, also when that region
    is the outermost container.
13. **Folds kept inside a fold (v6):** the folds behaviours 11 and 12 keep inside a folded region are
    not shown while it is folded. When the outer fold is unfolded, they are folded again, outermost
    first; when a caret or a jump opens the outer fold (behaviour 4), those of them that would hide the
    target line are dropped and the others are folded. They move with edits as their outer fold does
    and go when an edit unfolds it (behaviour 5). `CollapseRegion`, a click on a gutter mark and
    `CollapseAllRegions` absorb inner folds as before (behaviour 3) and keep nothing inside.
    `ExpandAllRegions` drops them all. [GEN]
14. **Find Action (v6):** the actions are listed under Folding as Expand Recursively, Collapse
    Recursively, Expand All to Level 1…5 and Expand to Level 1…5 (SPEC-0031). They have no MCP tool:
    folding stays a view state (behaviour 6). [GEN, ADR-0008] **(v8)** `editor_action` answers
    them, see behaviour 23.

15. **Fold Code Block (v7; `CollapseBlock`, ⇧⌘. / Ctrl+Shift+.):** in a Java file, folds the
    innermost code block around the caret that is still open.
    - **Blocks:** the `{…}` blocks of code — the bodies of methods, constructors, lambdas and
      initialisers, the blocks of statements and the body of a `switch` — not the body of a class,
      interface, enum or record. A block is around the caret from before its `{` to behind its
      `}`.
    - **The walk:** from the innermost block outwards, a block on one line, a block that is
      folded as exactly its lines and a block that lies inside a fold are passed over; the first
      other block is folded from the line of its `{` to the line of its `}`, and folds inside it are absorbed (behaviour 3). Used
      again, the action therefore folds the next block outwards. When that block partly overlaps
      a fold — neither lies inside the other — the walk ends and nothing is folded.
    - **Caret:** a caret the fold would hide goes to the end of the fold's first line, as for
      `CollapseRegion`.
    - In other languages nothing changes and the status bar says that the action knows the code
      blocks of Java only; so it does while the file has no syntax tree.
    [OSS, `CollapseBlockHandlerImpl.java`, `JavaCollapseBlockHandler.java`,
    `CollapseBlockAction.java`; GEN: by lines, the status message]
16. **Fold Selection / Remove Region (v7; `CollapseSelection`, ⌘. / Ctrl+.):**
    - **With a selection** at the primary caret: the lines from the one the selection starts on
      to the one its last character is on (a line break at the selection's end is not counted)
      become a folded region of their own, whatever the syntax: the first line stays, the others
      are hidden, folds inside are absorbed. The selection is dropped and the caret goes to the
      end of the first line. When exactly these lines are folded already, that fold is opened
      instead. A selection within one line cannot be folded, and one whose lines partly overlap
      a fold is not folded; the status bar says which.
    - **Without a selection:** the fold on the caret's line is opened; when there is none, the
      innermost foldable region around the caret is folded as `CollapseRegion` folds it — a
      region that partly overlaps a fold made from a selection is left out; without either
      nothing happens.
    [OSS, `CollapseSelectionHandler.java`, `FoldingUtil.java`; GEN: by lines, the status
    messages]
17. **Folds of these actions (v7):** they are folds like the others — the gutter mark, the
    placeholder, a click, a caret or jump into them and edits act on them as behaviours 2, 4 and 5
    say. Since a fold from a selection need not follow the syntax, no action folds lines that
    partly overlap a fold; the rows on screen stay those of behaviour 1. They last while they are folded: a region made from a selection is gone once it is
    opened, and no fold is kept when the tab is closed or for the next session (as before).
    Find Action lists the actions under Folding as Fold Code Block and Fold Selection / Remove
    Region; UI automation knows that they change nothing outside the window (SPEC-0319).
    `editor_action` (SPEC-0057) answers `fold_block` and `fold_selection` with `fold`, the first
    and last line (1-based) the action would fold in the saved file with nothing folded, or
    `null`; `fold_block` on a file that is not Java is an error; folding itself stays a view state (behaviour 6). [GEN, ADR-0008]

18. **Outermost container (v8, FO1):** a file may have one outermost container, the region that
    holds nearly everything else:
    - **Java and Kotlin:** the body of the type — class, interface, enum, record, annotation
      type, object — when the top level of the file declares exactly this one type and nothing
      else beside the package line, the imports and comments. A file with two top-level types,
      or a Kotlin file with a type and a top-level function or property, has none.
    - **JSON:** the root object or array.
    - **HTML and XML:** the root element, when there is exactly one.
    - **Other languages** (Rust, Python, Go, JavaScript, TypeScript, C, C++, PHP, Ruby, CSS,
      YAML, TOML, Markdown, shell scripts and the rest) have none: their files hold several
      things side by side.

    `CollapseAllRegions` folds every region but this one, so the file still shows what it holds:
    in a Java class the comments, the imports, the custom regions, the runs of line comments,
    the methods, the anonymous and the inner classes are folded and the class body is open. An
    outermost container that was folded by hand stays folded. For `ExpandAllToLevelN` the
    outermost container is no level (behaviour 12). The container stays a region for everything
    else: its gutter mark, `CollapseRegion` with the caret directly in it, `ExpandToLevelN` and the
    recursive actions. [OBS-0044 FO1 for the Java class; GEN for the other languages and for
    files with several types]
19. **Custom regions (v8, FO2):** two comments make the lines from the first to the second a
    foldable region of kind `region`:
    - **Start:** a comment whose text, behind the comment sign, is `region` and optionally a
      name — `// region Name`, `//region Name`, `# region Name`, `#region Name`, `-- region Name`,
      `/* region Name */`, `<!-- region Name -->`, also with a `#` before the word
      (`// #region Name`) — or `<editor-fold desc="Name">` (other attributes are ignored, the
      name may be missing).
    - **End:** `endregion` in the same forms (text behind it is ignored) or `</editor-fold>`.
    - A marker is a comment on one line with nothing else on that line. `region` and
      `endregion` are whole words: `// regional` is no marker. Which comment signs there are
      follows the language's syntax tree; a file without one (plain text) has no regions.
    - **Pairs:** an end closes the nearest start before it that is still open, so regions nest.
      A marker without a partner — a start that is never closed, an end without a start — folds
      nothing and is no error; the other pairs of the file still fold. A pair on one line folds
      nothing.
    - A region that crosses another region — it starts inside a block and ends behind it, or the
      other way round — folds nothing: folds never overlap partly (behaviour 17).
    - Marker lines are no part of a run of line comments (SPEC-0030 behaviour 3); on its first
      line the custom region is the region the gutter mark and the actions take.
    - Folded, the region shows its name in place of the start marker (behaviour 21). It takes
      part in everything else as any region: Collapse All, levels, the recursive actions,
      `folding_ranges`. [OBS-0044 FO2 for `// region Name` … `// endregion` in Java; GEN for the
      other forms and languages and for unbalanced and crossing markers]
20. **Documentation comments (v8, FO3):** `CollapseDocComments` folds every documentation
    comment of the file that spans more than one line, `ExpandDocComments` unfolds them — also
    those kept as folded inside a folded region (behaviour 13), which then stay open when that
    region is opened. Other folds stay as they are. A documentation comment is a block comment
    that starts with `/**` (Java, Kotlin, JavaScript, TypeScript, C, C++, PHP and the other
    languages with such comments) or a run of line comments that all start with `///` or `//!`
    (Rust). A file without one is left as it is. [GEN; the actions are named in OBS-0044 FO3,
    their effect was not observed]
21. **Placeholders (v8, FO4):** a folded region shows what it is. Three dots (`...`) stand for
    the hidden text; before v8 every placeholder was `…` behind the whole first line.
    - **Block:** when the first line ends with the opening bracket and the last line starts with
      its partner, the placeholder is the three dots and the closing bracket right behind the
      opening one, and both brackets and the dots are on the `FOLDED_TEXT_ATTRIBUTES`
      background: `void add(String name) {...}`, `[...]`, `(...)`. A block without brackets (an
      indentation block, an element of HTML or XML) shows the three dots a little apart.
    - **Block comment:** the opening sign, the three dots and the closing sign: `/*...*/`,
      `<!--...-->`. Text behind the opening sign on the first line is not shown.
    - **Documentation comment:** the opening sign, the first sentence, the three dots and the
      closing sign: `/** A class to fold. ...*/`. The first sentence is the comment's text without
      the `*` at the line starts, lines joined by one space, up to the first full stop that is
      followed by a space or the end; without such a full stop up to the first empty line or the
      first line that starts with `@`. A comment without text reads `/**...*/`. A run of `///`
      lines reads `/// First sentence. ...`.
    - **Imports:** the keyword and the three dots: `import ...` (`use ...`, `from ...`,
      `#include ...`).
    - **Run of line comments:** the comment sign and the three dots: `//...`, `#...`.
    - **Custom region:** its name in place of the marker, at the marker's indentation; a region
      without a name shows the three dots.
    - **One-line methods** keep the placeholder of behaviour 7 (the statement and the closing
      brace, in the code's colours), also when they are folded by hand or by an action.
    - **A fold made from a selection** (behaviour 16) is no region of the syntax and shows the
      three dots behind its first line.
    - **The rest of the first line:** where the placeholder stands for a part of the first line
      (imports, comments, custom regions), that part is not drawn. The caret can stand in the
      part that is shown. After folding, a caret behind it goes to its end; a caret that moves
      into the part that is not shown — by a key, a jump or a selection — opens the fold, as a
      caret on a hidden line does (behaviour 4); a click behind the shown part opens it too
      (behaviour 2).
    - **`} else {`:** a block that ends on the line where the next block starts ends, as a
      region, on the line before. So the `if` block and the `else` block are two regions, both
      can be folded, and the line with `} else {` stays; the first block then shows the three
      dots without a closing bracket. Before v8 the second block was no region of its own.
    - **Where a block starts:** a block whose opening bracket stands behind other text on its
      line starts on that line — a method with an annotation on the line above folds from the
      line of its signature's `{`, not from the annotation. A bracket alone on its line still
      folds from the line of its header.
    - `folding_ranges` gives the placeholder of every region as `collapsed_text` and, where it
      stands for the rest of the first line, `keep`, the number of characters of that line that
      stay (SPEC-0030 v3). [OBS-0044 FO4 for imports, block comment, documentation comment,
      block and custom region in Java, and for `if` and `else` as two blocks; GEN for the rest]
22. **Folding arrows (v8, FO5):** the gutter mark of a folded region (▸) is always shown. The
    mark of an open region (▾) is shown while the pointer is over the editor — its gutter or its
    text — and not otherwise. A click on the first line's place in the folding column folds the
    region in either case; the pointer is over the gutter then, so the mark is there.
    Setting: Editor › General › Code Folding › *Show code folding arrows*: `always` or
    `on hover` (the default, as observed); Enter, Space or a click switches. `always` shows the
    marks of open regions all the time, as before v8. In the settings file it is
    `folding_arrows = "always" | "hover"` under `[editor]` (settings schema, SPEC-0303), and
    `folding_arrows` in `set_ide_settings`; open editors follow at once. [OBS-0044 FO5: marks only
    at folded regions and under the pointer; the reference's own setting was not looked at, GEN]
23. **Menu, Find Action, keys, agents (v8, FO3):** Code › Folding holds every folding action, in
    groups: Expand, Collapse · Expand Recursively, Collapse Recursively · Expand All, Collapse
    All · Expand to Level › 1…5, Expand All to Level › 1…5 · Expand Doc Comments, Collapse Doc
    Comments · Fold Selection / Remove Region, Fold Code Block. The editor's context menu has
    Expand, Collapse, the recursive pair, Expand All, Collapse All and the documentation comment
    pair. Find Action lists all of them under Folding (behaviours 14 and 17; new: Expand Doc
    Comments, Collapse Doc Comments). The keys are those of the bundled keymaps
    (docs/research/KEYMAP_COVERAGE.md): ⌥⌘= / ⌥⌘- for the recursive pair, ⌥⌘* then 1…5 for
    "all to level", ⌘* then 1…5 for "to level"; the documentation comment actions have no key
    there and none here. `*` is the key of the numeric keypad, as in the keymaps (`MULTIPLY`).
    These are shortcuts of two keystrokes: a first keystroke that is bound only as the start of
    such shortcuts does nothing by itself and waits; the next keystroke in the editor picks the
    action and types nothing; a key that completes no shortcut acts as itself. Before v8 the
    editor knew neither the key nor the second keystroke, so the level shortcuts could not be
    pressed (`keymap::tests::the_level_shortcuts_start_with_a_key_of_their_own`, AK-28). UI automation knows that every folding action changes nothing outside
    the window (SPEC-0319).
    `editor_action` (SPEC-0057) carries the actions for agents as `collapse_all`, `expand_all`,
    `collapse_recursively`, `expand_recursively`, `expand_all_to_level` and `expand_to_level`
    (with `level`, 1 to 5; anything else is an error), `collapse_doc_comments` and
    `expand_doc_comments`. Each answers `folds`: the regions that are seen folded after the
    action in the saved file when nothing was folded before, in the order of their lines, each
    with `first_line` and `last_line` (1-based) and `collapsed_text`. The expanding actions
    therefore answer an empty list. The first caret says where `collapse_recursively` and
    `expand_to_level` work. As for `fold_block` and `fold_selection`, nothing is written and no
    window is touched: folding stays a view state (behaviour 6). [GEN, ADR-0008]
24. **Custom Folding… (v9):** `GotoCustomRegion` (⌥⌘. in the bundled macOS keymap,
    Ctrl+Alt+. in the default one; Navigate › Custom Folding…, SPEC-0124 v14 behaviour 16)
    opens a list of the custom regions of the active file.
    - **Rows.** One row per custom region of behaviour 19, in the order of their first lines,
      a region inside another one directly after it: the region's name and the number of its
      first line (`Fields  (line 14)`); a region without a name is listed as
      `Unnamed region`. The list holds exactly the regions that fold: a marker without a
      partner and a region that crosses a block are not in it.
    - **Selected first.** The innermost region around the caret; else the first region that
      starts behind the caret; else the last one.
    - **Choosing.** ↑, ↓, Home and End (SPEC-0275 v19 behaviour 47) move, typing narrows the
      rows, Enter or a click puts the caret on the first line of the chosen region, in front
      of the comment sign of its marker, opens a fold the line lies in (behaviour 4) and
      gives the focus back to the editor. The place left is remembered, so Back returns to it
      (SPEC-0071). Escape closes the list and moves nothing.
    - **Without regions.** In a file without custom regions — also a plain text file, which
      has none (behaviour 19) — no list opens and the status bar says that the file has no
      custom folding regions. Without an editor the action is not available.
    - The regions are read from the syntax tree of the text as it is in the editor, saved or
      not; right after a file opened, before it is parsed, the list may still be empty.
    - UI automation knows that the action changes nothing outside the window (SPEC-0319).
      Agents get the same regions from `folding_ranges` (kind `region`, SPEC-0030); there is
      no further tool.
    [OSS: the id, the title "Custom Folding…" from `ActionsBundle.properties` and the chords
    of the bundled keymap files; GEN: a list to choose from. What the reference's list shows
    for a region without a name and which row it selects first was not observed.]

## Acceptance criteria
- **AK-1** Row/line mapping both ways, folds absorbing inner folds, reveal, edits shifting and
  unfolding (unit tests of `fold_model`).
- **AK-2** In a real window (`idecraft ui --selftest`): ⌘- inside `main` folds its body and keeps the
  caret on the first line; ⌘= unfolds it.
- **AK-3** Use by hand: gutter marks and placeholder clicks, ⇧⌘- / ⇧⌘=, scrolling past folds.
- **AK-4 (v2)** The syntax tree finds Java bodies of one statement on one line with their placeholder
  and skips longer bodies, bodies on the signature's line, an opening brace alone on its line, a
  comment as the only content and other languages; a fold's own placeholder follows it through
  edits and goes when it is unfolded or absorbed (unit tests).
- **AK-5 (v2)** In a real window (`idecraft ui --selftest`): opening the Java insight file folds both
  one-line methods with their statements as placeholders; `folding_ranges` gives the
  `collapsed_text` (protocol test).
- **AK-6 (v3)** The placeholder's statement takes the colours of the hidden line by character
  (tabs and non-ASCII text included), the closing brace is found, and other placeholders are left
  as before (unit test `fold_model::tests::placeholder_keeps_the_statements_colours`).
- **AK-7 (v3)** In a real window (`idecraft ui --selftest`): both folded methods of the Java insight
  file are drawn with at least three text colours in the placeholder and two brace backgrounds.
- **AK-8 (v4)** The import block of a Java file spans its groups with the comment between them, a Kotlin
  file's likewise; one import, imports on one line, no import and other languages give none (unit test
  `syntax::tests::java_and_kotlin_import_blocks`).
- **AK-9 (v4)** Whole lines put in after a fold's last line or before its first line leave it folded with
  its placeholder and move the folds and rows below; lines put inside a fold open it
  (`fold_model::tests::inserted_lines_keep_the_folds_beside_them`); which insertions add whole lines only
  (`decl_marks_model::tests::whole_lines_put_in_keep_the_marks_beside_them`).
- **AK-10 (v4)** In a window (`crates/workbench/tests/it/jvm_editor_rest.rs`): opening a Java file with three
  imports in two groups folds lines 3–6 and changes no text, Expand All opens them; a Kotlin file's two
  imports fold; a file with one import and one without stay open
  (`the_import_block_is_folded_when_a_file_opens`); with the setting off nothing folds
  (`the_setting_keeps_the_imports_open`). Not run here: the window self-test, whose Java steps now open
  files with folded imports.

- **AK-11 (v5)** `decl_marks_model::tests::replacements_that_only_put_lines_in` (the jdtls form, a pure
  insertion, a changed statement, a deletion, multi-byte characters).
- **AK-12 (v5)** In a window (`crates/workbench/tests/it/jvm_editor_rest.rs::a_one_line_method_stays_folded_when_methods_are_added`,
  macOS): a one-line method folded on open stays folded when a method is added in the jdtls form
  through the editor's text edits. Written on Linux, to be run on the Mac.
- **AK-13 (v6)** Folds kept inside a fold come back when the outer one is unfolded, outermost first;
  a reveal drops only those hiding the line; they move with lines put in above and go with an edit
  inside; a plain fold keeps nothing (`fold_model::tests::folds_kept_inside_a_fold`).
- **AK-14 (v6)** Levels of nested regions, the folds of "all to level N" and of "to level N" in one
  region, and the region the recursive actions work on (`fold_model::tests::levels_of_regions`).
- **AK-15 (v6)** In a window (`crates/workbench/tests/it/editor_actions_keymap.rs::folding_recursively_and_to_a_level`):
  on a Java class with two methods and an inner block, the actions leave the folded regions
  behaviours 11 and 12 name; the ids are bound in the macOS keymap and titled in Find Action
  (`everywhere::tests::editor_actions_from_the_keymap_have_titles`).

- **AK-16 (v7)** `syntax::tests::java_code_blocks_around`: method, loop and `if` blocks around an
  offset, innermost first; the block whose `}` is before the caret; a lambda's and a `switch`'s
  block; no class body; no blocks in a Python file.
- **AK-17 (v7)** `fold_model::tests::fold_code_block_goes_outwards` (the innermost block, the next
  one on a second use, a one-line block passed over, a partly overlapping fold ends the walk) and
  `fold_model::tests::fold_selection_by_lines` (new region, the same lines again open it, one
  line, partial overlap, folds inside absorbed, without a selection the fold on the line or the
  region around it).
- **AK-18 (v7)** In a window, through the window's key dispatch
  (`crates/workbench/tests/it/editor_actions_fifth_package.rs::folding_a_block_and_a_selection`):
  ⇧⌘. in the `if` of a Java method folds it, again the loop, again the method; ⌘. with three
  selected lines folds them and with the caret on that line opens them; a text file says that
  there are no code blocks. Titles and effects:
  `everywhere::tests::fifth_package_actions_have_titles`,
  `security::tests::fifth_package_actions_declare_their_effects`; `editor_action`:
  `protocol.rs`: `editor_action_fifth_package`.
- **AK-19 (v7)** Hand test, open: the keys on a real keyboard, the folds on screen, both actions
  next to the reference.

- **AK-20 (v8)** The outermost container by language — one type in a Java or Kotlin file, the
  root value of JSON, the root element of HTML and XML; none for two types, for a Kotlin type
  beside a function, for Rust and Python — and what Collapse All folds beside it
  (`folding::tests::outermost_containers`, `folding::tests::outline_of_a_java_file`).
- **AK-21 (v8)** Which comments are markers in every form of behaviour 19, words that only begin
  like one, pairs innermost first, markers without a partner, a marker behind code, a region
  that crosses a block, marker lines outside runs of comments; Java, Python, HTML and Rust
  (`folding::tests::comments_that_mark_regions`, `markers_pair_innermost_first`,
  `custom_regions_from_comments`).
- **AK-22 (v8)** On the Java file of OBS-0044: "all to level 1" folds the `if`, the `else`, the
  method of the anonymous class and the argument list in the inner class, "all to level 2" the
  `for` and the lambda, level 3 nothing; "to level" in a block, in a method and in the class
  body; the documentation comment is found (`folding::tests::outline_of_a_java_file`,
  `levels_of_regions`).
- **AK-23 (v8)** The first sentence; the placeholders of block comments, documentation comments,
  runs of line comments and `///` lines, imports, blocks with and without their closing bracket,
  custom regions and a one-line method, with the characters of the first line that stay
  (`folding::tests::placeholders_of_comments_and_blocks`, `outline_of_a_java_file`). A fold takes
  the look of its region, a fold with its own placeholder or from a selection does not, looks
  move with edits (`fold_model::tests::folds_take_the_look_of_their_region`).
- **AK-24 (v8)** Code › Folding holds the actions of behaviour 23 in their groups
  (`menus::tests::folding_actions_are_in_code_folding`); Find Action titles the documentation
  comment actions (`everywhere::tests::editor_actions_from_the_keymap_have_titles`); every
  folding action declares that it changes the view only
  (`security::tests::fifth_package_actions_declare_their_effects`); `folding_ranges` and the
  eight actions of `editor_action` with their `folds`, the level's bounds (`protocol.rs`:
  `folding_ranges_of_a_file`, `folding_actions_and_custom_regions`); `folding_arrows` in
  `set_ide_settings` and in the settings schema (`protocol.rs`: the settings test;
  `format::tests::published_schemas_match_the_shapes`).
- **AK-25 (v8)** In a window
  (`crates/workbench/tests/it/folding_levels_regions_placeholders.rs::collapse_all_levels_and_doc_comments`):
  on the Java file of OBS-0044 Collapse All folds nine regions and leaves the class body open;
  the nine folded lines are drawn as behaviour 21 says (`/*` and `...*/`, `import ` and `...`,
  `/**` and ` A class to fold. ...*/`, the indentation and `Fields`, `//` and `...`, the one-line
  method with its statement, three blocks with `...}`); all to level 1, 2 and 3; to level 1 in a
  method; the documentation comment alone, and opened out of everything folded; no text changes.
- **AK-26 (v8)** In a window (`…::custom_regions_and_the_caret`): Collapse in a custom region
  folds it and puts the caret in front of the name; a caret moved into the rest of that line
  opens it; Collapse in the imports puts the caret behind the keyword; in a file with a start
  marker without an end only the class body folds, and Collapse All folds nothing there.
- **AK-27 (v8)** In a window (`…::folding_arrows_follow_the_pointer`): without the pointer only
  the two regions folded on open have an arrow; with the pointer over the editor every region
  in view has one; with the setting on `always` they are there without the pointer.
- **AK-28 (v8)** In a real window (`idecraft ui --selftest`, `folding_steps`): the steps of AK-25
  to AK-27 on a file of the self-test's project — Collapse All from the Code menu's action,
  the placeholders as drawn, a custom region's name, all to level 1, the documentation comments,
  the arrows with the pointer over the editor and away from it.
- **AK-29 (v8)** Hand test, open: Collapse All, the levels, the placeholders and the arrows next
  to the reference on the file of OBS-0044; the keys on a real keyboard.
- **AK-30 (v9, behaviour 24)** The rows of the list are the regions that fold, nested ones
  behind the region around them, with their names, `Unnamed region` and their line; a marker
  without a partner gives no row; the row selected first is the innermost region around the
  caret, else the next, else the last (`folding::tests::custom_regions_are_listed_for_the_chooser`).
  In a window of the test harness, on a Java file with three regions: the list has the title
  and the three rows, the region around the caret is selected, Home, ↓ and Enter put the caret
  in front of the marker of the second region and the editor has the focus, Back returns to
  the place left, Escape moves nothing, and a file without regions opens no list and says so
  (`seventh_package_keymap.rs::custom_regions_are_chosen_from_a_list`). The action is in
  Navigate and Find Action with its keys in both bundled keymaps
  (`menus::tests::seventh_package_items_are_in_their_menus`,
  `everywhere::tests::seventh_package_actions_have_titles`) and declared to UI automation
  (`security::tests::seventh_package_actions_declare_their_effects`). On screen and next to
  the reference: not checked.

- **AK-31 (v10, behaviour 7a)** `syntax::tests::one_line_bodies_follow_the_open_conditions`: a
  body whose folded line passes the right margin and a method whose parameter list ends on another
  line than its name stay open; `int totalCents() { return qty * unitPriceCents; }` folds; the open
  default of the setting is on (`ide_settings` default).

## Out of scope
Folding comments (a licence header) by default when a file opens (one-line methods are folded since v2, imports of Java and Kotlin files since v4), import folds on open for other languages, remembering folds across sessions.
v7: a fold that begins or ends inside a line, `CollapseBlock` for languages other than Java,
regions from a selection that stay as open regions with a gutter mark.
v8: region markers that are no comments (`#pragma region` in C and C++, `#region` directives of
languages without a grammar here), regions that are folded by default when a file opens
(`defaultstate="collapsed"`), documentation that is no comment (Python docstrings), a tooltip
that shows the folded text under the pointer, rendered documentation comments, a setting for
what is folded on open beyond behaviours 8 and 10, a limit for the length of the first sentence.

## Known deviations from the reference
The arrow keys unfold a region they enter instead of stepping over it; the placeholder is `…` and does
not repeat the region's brackets. Not compared side by side yet.

v4 (OBS-0027 L19): the reference shows the folded imports as the word `import` and an ellipsis; IDECraft
keeps the whole first import line and puts the ellipsis after it. An import added while the block is
folded opens the block.

v10: OBS-0056 saw the reference show short Java methods unfolded where IDECraft folds them;
IDECraft follows the open platform's default (behaviour 7a). Whether the reference's own folding
setting is switched off was not read (PO-STATUS question 44).

v6: not compared with the reference. Open there: whether its levels count the same regions (IDECraft
counts every foldable region of the syntax tree, also comments and import blocks), and which region
`ExpandToLevelN` and the recursive actions take when the caret stands in nested regions (IDECraft:
the innermost, as `CollapseRegion`).

v7: taken from the open platform's sources, not compared with the reference IDE.
- Folds are whole lines here. The open actions fold from the `{` to the `}` of a block and from
  the first to the last selected character, so text before and after stays on the line with a
  `{...}` or `...` placeholder between; IDECraft keeps the whole first line, ends it with `…` and
  hides the following lines whole — also what stands behind the closing brace or behind the
  selection on its last line. A block or selection inside one line is not folded at all.
- The open `CollapseSelection` asks in a dialog before it removes folds a selection partly
  overlaps and then folds; IDECraft folds nothing and says why. It refuses to remove a region of
  the syntax that has exactly the selected range; IDECraft opens it.
- A region made from a selection lives on in the open editor as an open region that can be
  folded again; here it is gone once opened.
- After folding, the open actions put the caret behind the folded text; IDECraft leaves a
  visible caret where it is and moves a hidden one to the end of the fold's first line.
- `CollapseBlock` is carried out for Java only: the open sources hold a block handler for Java
  and none for the other languages; what the reference's other language plugins do is not known.

v8 (OBS-0044): compared on one Java file; the earlier notes on `…` and on the first import line
no longer hold (behaviour 21).
- Folds are still whole lines. The reference shows `if (a) {...} else {...}` on one row; IDECraft
  shows `if (a) {...` and below it `} else {...}`. Text behind a block's closing bracket on its
  last line (`});`) is hidden with the block.
- Levels count every foldable region of the syntax tree. Beside the `if` and the `else` that
  OBS-0044 lists for "all to level 1", IDECraft also folds what is equally deep: the method of
  an anonymous class and a call's argument list over several lines (a region here since
  SPEC-0030, probably none in the reference). "All to level 2" matched for the `for` and the
  lambda. Whether the reference keeps the regions inside a folded one folded was not observed.
- The outermost container stays foldable by hand here; whether the reference offers that for
  the only class of a file was not observed. The rule for files with several top-level types
  and the rules for Kotlin, JSON, HTML and XML are IDECraft's own.
- Custom regions: only `// region Name` … `// endregion` in Java was observed; the other
  markers, nesting, unbalanced and crossing markers follow behaviour 19 without comparison.
- The documentation comment actions were not carried out in the reference.
- The first sentence of a documentation comment was compared for one comment; the exact spaces
  around the three dots were not compared character by character.
- The arrows: the reference's setting was not looked at; `on hover` here means the pointer
  anywhere over the editor.
