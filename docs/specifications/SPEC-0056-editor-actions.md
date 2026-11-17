# SPEC-0056: Editing actions

- **Version:** 6 · **Date:** 2026-11-16 (v6: Tab out of a closing bracket or quote, behaviour 22, AK-17; eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v5: Complete Current Statement, behaviour 20, and Unwrap/Remove, behaviour 21, AK-15 and AK-16; eighth package of docs/research/KEYMAP_COVERAGE.md; v4: move an element of a list left or right; behaviour 19.
  v2: Enter inside the indentation keeps the whitespace before the
  caret on the upper line; OBS-0012. v3: side by side in OBS-0018 — caret on a duplicated declaration,
  moving statements, join removing braces, comment caret, block comments, Markdown comments, `{`
  closed on Enter; behaviours 12–18)
- **Sources:** GEN (the everyday editing actions of IDEs: duplicate line, delete line, move line up/down,
  join lines, comment with line comment, indent/unindent, Enter with smart indentation, automatic closing
  of brackets and quotes, matching brace; F-010), SPEC-0003 (buffer and undo), SPEC-0025 (carets),
  SPEC-0004 (comment tokens)
- **Feature matrix rows:** F-010

## Purpose
The editing operations users press dozens of times an hour, working on every caret at once and
undoable as one step.

## Behaviour
1. **Duplicate:** a selection is inserted again right after itself and stays selected on the copy; an
   empty caret duplicates its line below and moves to the copy at the same column. [GEN]
2. **Delete lines:** the lines the carets touch are removed (on the last line, the break before it); the
   caret keeps its column on the line that takes its place. [GEN]
3. **Move lines up/down:** the carets' line blocks swap with the line above or below, carets and
   selections travel with them; nothing happens when a block is at the document edge. [GEN]
4. **Join lines:** the line break and the next line's indentation become one space; no space after an
   opening `(`/`[` or before `)`, `]`, `}`, `,`, `;`, `.`, and none when one side is empty. [GEN]
5. **Toggle line comment:** if every non-blank line of a block starts with the token (after
   indentation), the token (and an added space) is removed; otherwise every non-blank line is
   commented, by default at column 0 without a space (the reference's default for line comments),
   optionally at the block's smallest indentation with a space. Blank lines are left alone unless the
   whole block is blank. [GEN, SPEC-0004]
6. **Indent/unindent:** non-blank lines of the selection get one indentation unit; unindent removes one
   unit, a tab, or the leading spaces up to the unit's width. [GEN]
7. **Brackets and quotes:** typing `(`, `[`, `{`, `"`, `'` inserts the closer when the next character is
   whitespace, a closer or the end; a quote right after a word character is typed alone (e.g. `don't`);
   with a selection the pair surrounds it and the text stays selected; typing a closer that is already
   next to the caret steps over it; backspace between an empty pair removes both. [GEN]
8. **Enter:** breaks the line keeping the line's indentation, drops trailing whitespace before and leading
   whitespace after the caret, adds one indentation unit after `{`, `(`, `[` or `:`, and between a
   bracket pair puts the closer on its own line at the original indentation. [GEN]
   **v2:** when only whitespace stands before the caret (the caret is inside the line's leading
   indentation), that whitespace stays on the upper line instead of being dropped, so the caret column
   is the same on both lines and moving up lands where Enter was pressed; the lower line still starts
   with the line's full indentation followed by the rest without its leading whitespace. On a line that
   holds only whitespace the same applies. [GEN; OBS-0012: IDECraft left the upper line empty]
9. **Matching bracket:** the bracket at (or just before) the caret is matched forward or backward,
   skipping brackets inside string literals. [GEN]
10. Every action applies to all carets, merges carets that end up on the same place, and is one undo
    step; line breaks follow the document's dominant line ending. [GEN, SPEC-0003]
11. MCP tool `editor_action` previewing an action on a workspace file at given carets (1-based lines and
    columns) and returning text and carets; the file is not written. [ADR-0008]


### Version 3 (OBS-0018)
12. **Duplicate a declaration:** duplicating the single line of a Java local variable or field
    declaration without a selection (`Type name = …;` or `Type name;`, also with modifiers, generic
    and array types) puts the caret at the start of the name on the copy, so the duplicate name can
    be changed at once; other lines keep the caret's column (behaviour 1). [OBS-0018 item 4]
13. **Move statement (`MoveStatementUp`/`MoveStatementDown`):** in languages with braces the unit is
    the caret's line, or, if that line opens a block (ends with `{`), the line through its closing
    brace. Moving up (down mirrored):
    - blank lines between the unit and its neighbour stay where they are; the unit swaps with the
      previous sibling: a single line, or a whole block ending in a `}` line at the unit's
      indentation;
    - if that sibling is a block of a control statement (`if`, `else`, `for`, `while`, `do`, `try`,
      `catch`, `finally`, `switch`, `synchronized`) and the unit is a single line, the unit moves into
      that block (before its `}`, moving down: after its opening line) and gets the block's inner
      indentation;
    - if the line above is the opening line of the enclosing block: a control statement's block is
      left (the unit goes above the opening line with its indentation); a method, class or other
      declaration is not left, nothing happens;
    - carets move with the unit, their columns shifted by the change of indentation.
    In languages without braces the command moves lines (behaviour 3). [OBS-0018 item 5]
14. **Join removes braces:** joining a line that ends with `{` whose block holds exactly one line
    followed by its `}` line gives `opener statement` without the braces (`for (…) total += x;`).
    Joining two statements keeps one space after `;` (behaviour 4). [OBS-0018 item 6; IMP-0303: the
    reference joins `0;for` without a space]
15. **Comment caret:** a line comment toggled with one empty caret on one line moves the caret one
    line down, keeping its column, so that pressing again comments the next line; on a blank line the
    token is inserted and the caret stays after it. With a selection the selection stays. [OBS-0018
    item 7]
16. **Block comment (`CommentByBlockComment`):** with a selection the selected text is wrapped in the
    language's block comment tokens (`/*` `*/` in C-like languages, CSS and SQL; `<!--` `-->` in HTML,
    XML, Markdown, Vue, Svelte) and the selection covers the tokens; a selection that already starts
    and ends with them (spaces allowed) is unwrapped. Without a selection the tokens are inserted at
    the caret with the caret between them, or, if the caret is inside a block comment on its line,
    that comment is removed. Languages without block comments report the action as not handled.
    [OBS-0018 item 7]
17. **Comments in Markdown:** Markdown, HTML and XML have no line comment token; the line comment
    action wraps each non-blank line of the block in `<!-- ` and ` -->` and unwraps it when every
    non-blank line is wrapped. [OBS-0018 item 7; IMP-0304: the reference writes an empty link
    reference whose text sits in parentheses, inserts blank lines between commented lines and offers
    no block comment there]
18. **`{` closes on Enter:** typing `{` without a selection inserts only `{` (with a selection it
    still surrounds it); Enter right after a `{` at the end of a line whose braces in the document are
    not balanced (more `{` than `}`, braces in string literals and comments not counted) adds the
    closing `}` on its own line at the line's indentation, with the caret on the indented line in
    between. `(`, `[` and quotes stay as in behaviour 7. [OBS-0018 item 10]

### Version 4
19. **Move element (`MoveElementLeft` ⌥⇧⌘← / Ctrl+Alt+Shift+Left, `MoveElementRight` ⌥⇧⌘→ /
    Ctrl+Alt+Shift+Right):** the element of a comma-separated list the caret is in changes places with
    its left or right neighbour; the commas and the whitespace around them stay.
    - **Which list:** the innermost node of the syntax tree around the caret whose children are
      separated by `,` tokens with exactly one element between two commas (comments not counted;
      nothing after a trailing comma), and with the caret inside or at an edge of one element. This
      gives the arguments of a call, parameters, type arguments, the items of array, list, tuple, set
      and dictionary literals, enum constants and the like in every language with a grammar
      (SPEC-0027); a node that also holds other things beside its elements — a declaration with its
      type and several declarators — is not such a list.
    - **Effect:** the texts of the two elements are exchanged as one undo step; the caret keeps its
      place inside the moved element, a selection becomes a caret at its head. The first element is
      not moved left and the last not right. Without such a list, in plain text, or while the file has
      no syntax tree, nothing happens.
    - **Several carets:** every caret moves its element when the exchanges do not overlap; when two of
      them would touch the same element, only the primary caret's element moves.
    The syntax tree comes from the caller, as the selection stages of SPEC-0057 behaviour 13; MCP:
    `editor_keystrokes` carries the two actions out, and `editor_action` has `move_element_left` and
    `move_element_right`. [OSS (action ids), GEN, ADR-0008]
20. **Complete Current Statement (v5; `EditorCompleteStatement` ⇧⌘↩ / Ctrl+Shift+Enter):** on
    the line of the primary caret (other carets are dropped), in one undo step:
    - an open string literal of the line is closed with its quote, then the brackets `(` and `[`
      the line leaves open are closed in reverse order; brackets and quotes inside string
      literals and after a line comment do not count, and the closers go before a trailing line
      comment;
    - in a language with braces (behaviour 13), a line that starts with a control keyword (`if`,
      `else`, `for`, `while`, `do`, `switch`, `try`, `catch`, `finally`, `synchronized`, `when`,
      `match`, `loop`) and does not end with `{` gets ` {`, an empty line indented one level
      deeper with the caret on it, and `}` at the line's indentation; a line that ends with `{`
      gets the same inner line, and the `}` only when its block is not yet closed;
    - otherwise, in Java, JavaScript, TypeScript, C, C++, C#, PHP and Rust, a line that is not
      blank and does not end with `;`, `{`, `}`, `,` or `:`, is no comment, annotation or
      preprocessor line, gets `;`, and the caret starts a new line below at the line's
      indentation;
    - in Python a line that starts with a block keyword (`if`, `elif`, `else`, `for`, `while`,
      `def`, `class`, `try`, `except`, `finally`, `with`, `match`, `case`, also after `async`)
      and does not end with `:` gets `:`, and the caret starts a new line one level deeper;
    - in every other case (other languages, a line that needs nothing) the caret starts a new
      line below, indented as Start New Line (behaviour 6) does.
    MCP: `editor_action` with `complete_statement`. [GEN; DOC: the action adds the missing
    brackets and semicolon and puts the caret where the next statement starts]
21. **Unwrap/Remove (v5; `Unwrap` ⇧⌘⌦ / Ctrl+Shift+Delete):** in a language with braces, the
    blocks around the caret that can be unwrapped are listed innermost first, each as "Unwrap
    '<keyword>…'" (`if`, `else`, `for`, `while`, `do`, `try`, `synchronized`, `unsafe`, `loop`)
    or "Unwrap block" for a bare `{ … }`; the innermost is selected and Enter unwraps the chosen
    one in one undo step: the line that opens it and the line that closes it go, and its lines
    move left by the difference between their indentation and the opening line's. A block can
    be unwrapped when its `{` ends the opening line, the line holds nothing before the keyword
    but the indentation, and its `}` stands alone on the closing line (for `do`, followed by its
    `while (…);`, which goes too). Braces inside string literals and comments do not count. A
    block whose `}` is followed by `else`, `catch` or `finally`, and an `else` block that starts
    on the line of the `}` before it, are not offered. Without such a block, and in other
    languages, the status bar says "Nothing to unwrap here". MCP: `editor_action` with `unwrap`
    and `level` (1 = innermost, the default) answers the text after unwrapping and the
    `choices`. [GEN; DOC: the action removes the enclosing statement and keeps its body]
22. **Tab out of a pair (v6; `BraceOrQuoteOut` Tab):** with one caret and no selection, no
    completion list, template or parameter info, Tab moves the caret past the character after
    it when that character is `)`, `]` or `}` whose opening bracket stands before the caret on
    the same line, or a quote (`"`, `'`, `` ` ``) that closes a string opened before the caret on
    that line, and something other than indentation stands before the caret. Otherwise Tab does
    what it did (indent, accept an inline suggestion, a template's next place). Only where the
    keymap in use binds Tab to `BraceOrQuoteOut`. [OSS, GEN]

## Acceptance criteria
- **AK-1** Each action on single and multiple carets and selections with the expected text and caret
  positions, and one undo step restoring the original (unit tests per action).
- **AK-2** Comment toggling with `//` at column 0 and `#` at the indentation with a space, blank lines,
  mixed blocks.
- **AK-3** Bracket handling: auto-close, step over, surround, quote after a word, pair backspace;
  matching bracket across a string literal.
- **AK-4** The MCP tool comments a Rust and a Python file with the language's token, applies Enter
  between braces, finds a matching bracket, refuses a language without line comments and leaves the
  file unchanged.
- **AK-5 (v2)** Enter at column 9 of `        return x;` leaves eight spaces on the upper line and puts the
  caret before `return` at the same column below; Enter at column 5 keeps four spaces above (unit test
  `newline_inside_the_indentation`).

- **AK-6 (v3)** Duplicating `        String a = "Hello, " + name;` at column 12 puts the caret on `a`
  of the copy; `return b;` keeps the column (unit test `duplicate_puts_the_caret_on_the_declared_name`).
- **AK-7 (v3)** Move statement: a `for` block above the line before it; a method above the previous
  method with the blank line staying between; the first statement of a method body and the last
  member of a class stay; `return total;` below a loop moves into the loop and `total += x;` out of
  it, re-indented; Python moves lines (unit test `move_statement_follows_blocks`).
- **AK-8 (v3)** Join on `for (…) {` with one statement gives `for (…) total += x;`; join of two
  statements keeps a space (unit test `join_removes_the_braces_of_a_one_line_block`).
- **AK-9 (v3)** Comment caret moves one line down; on a blank line it stays after `//`; block comment
  wraps, unwraps, inserts `/**/` with the caret between; Markdown lines get `<!-- … -->` and lose it
  again (unit tests `comment_moves_the_caret_down`, `block_comments`, `markdown_comments`).
- **AK-10 (v3)** `{` alone stays alone; Enter after it adds `}` when the braces are unbalanced and
  not when they are balanced (unit test `brace_closes_on_enter`).
- **AK-11 (v3)** The MCP tool `editor_keystrokes` runs `MoveStatementUp` (`meta shift UP` in the
  macOS keymap) and `CommentByBlockComment` on a Java file (protocol test `editor_keystrokes_tool`).
- **AK-12 (v4)** The elements of the list around an offset: arguments of a Java call (nested call:
  the inner list inside it, the outer one on its name), Python parameters and a list with a trailing
  comma, a Rust array, JSON members; none for a Java field with two declarators and for a
  statement (unit test `syntax::tests::list_elements_around_an_offset`).
- **AK-13 (v4)** Exchange with the left and the right neighbour keeping the caret's place in the
  element, the edges, a selection, two carets in two lists, two carets on neighbours (only the
  primary moves), one undo step (unit test `commands::tests::move_element_left_and_right`).
- **AK-14 (v4)** MCP: `editor_keystrokes` with `alt shift meta RIGHT` in the macOS keymap and
  `editor_action` with `move_element_left` on a Java file; a file without a grammar is refused
  (protocol test `editor_actions_from_the_keymap`). In a window: Move Element right and left on
  the arguments of a call, each one undo step
  (`crates/workbench/tests/it/editor_actions_keymap.rs::methods_and_edit_locations`).
- **AK-15 (v5, behaviour 20)** Unit tests on lines of Java, Rust, JavaScript, C, Python and
  plain text: open calls, nested brackets, a string literal left open, brackets in strings and
  after comments, control lines with and without `{`, a line ending in `{` with and without its
  `}`, a complete line, a blank line, an annotation (`statement::tests::*`, `commands::tests::
  complete_statement_is_one_undo_step`); MCP `editor_action` with `complete_statement`
  (protocol test `editor_complete_statement_and_unwrap`); in a window of the test harness ⇧⌘↩
  completes a call in a Java file and opens a block after an `if`
  (`eighth_package_keymap.rs::statements_are_completed_and_blocks_unwrapped`).
- **AK-16 (v5, behaviour 21)** Unit tests: `if`, `for`, nested blocks innermost first, `do …
  while`, a bare block, braces in strings and comments, `} else {` and `} catch` not offered,
  tabs and spaces (`statement::tests::*`); MCP `editor_action` with `unwrap` and `level`
  (protocol test above); in a window, ⇧⌘⌦ lists the blocks, Enter unwraps the chosen one and
  Undo restores it (same window test).
- **AK-17 (v6, behaviour 22)** Unit test
  `workbench_keymap_eleventh::tests::tab_steps_out_of_a_pair_on_the_line` (round, square and
  curly brackets, quotes, nested pairs, not after indentation only, not without the opening
  character on the line); window test `eleventh_package_keymap.rs`:
  `tab_steps_out_of_pairs_and_expands_a_template` (macOS).

## Out of scope
Language-aware indentation from the parse tree (the formatter, SPEC-0043, covers
reformatting), live templates, smart paste, camel-hump word motion, the editor view (M3.7).
v5: Complete Current Statement and Unwrap/Remove are behaviours 20 and 21; out of scope stay
statements over several lines, the "Remove" entries and the constructs behaviour 21 does not
offer.

## Known deviations from the reference
Compared side by side in OBS-0018 (2026-11-05, Java, Markdown, plain text). Deviations on purpose:
join keeps a space after `;` (IMP-0303); Markdown comments are HTML comments and a block comment is
offered there (IMP-0304). Moving statements follows braces and control keywords, not a parse tree;
blocks of other kinds (lambdas, anonymous classes) are moved as a whole.

v4: not compared with the reference. Open there: which lists its Move Element actions know per
language (IDECraft: every comma-separated list of the syntax tree) and where they leave the caret.
- v5: Complete Current Statement and Unwrap/Remove read the lines, brackets and keywords, not a
  syntax tree, so they follow the code's layout: a statement over several lines is completed on
  the caret's line only, and Unwrap offers only blocks laid out one brace per line end. The
  reference also offers "Remove" entries (the block with its body) and more constructs
  (`else` branches, `catch`, lambdas, conditional expressions); IDECraft does not yet. Not
  compared next to the reference.
- **Tab out of a pair (v6):** the reference steps out only of a pair it closed itself while
  the person typed. IDECraft keeps no record of auto-closed pairs and steps out of any pair
  whose opening character is on the caret's line, so Tab before a `)` typed by hand also steps
  over it; a Tab meant as indentation inside a pair on one line is rare.
