# SPEC-0077: Quick Documentation from the editor

- **Version:** 10 · **Date:** 2026-11-18 (v10: character references such as `&lt;` are shown as their characters, behaviour 2, AK-15; OBS-0097 HC5; v9: `_` and `__` emphasis lose their markers, an embedded image (`data:`) is left out, behaviour 2, AK-14; OBS-0087 W2); 2026-11-16 (v8: External Documentation, behaviour 7, AK-13; eleventh package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v7: Type Info (`ExpressionTypeInfo`), the type of the expression at the caret
  from the language server's hover, behaviour 6, AK-12; seventh package of docs/research/KEYMAP_COVERAGE.md; v2: links show their text, repeated documentation once; hand test Nr. 110;
  v3: links whose target holds unbalanced parentheses, labelled sections, inline code; OBS-0012;
  v4: single-star emphasis removed, OBS-0016; v5: jdtls signature as code with one parameter per line,
  owner above it, no empty parameter list, module name without the server's project id, OBS-0016 defect F034-2; v6: documentation on mouse
  hover, hand test Nr. 67 window run 2026-11-10)
- **Sources:** GEN (documentation of the symbol at the caret in a popup, opened by shortcut; F-034;
  IDEs also show it when the pointer rests on a symbol), DOC (the reference's help describes
  quick documentation shown on mouse hover, on by default),
  OSS (action id `QuickJavaDoc` with F1 / ⌃J on macOS and Ctrl+Q elsewhere in the ported keymaps),
  STD (LSP `textDocument/hover`, Markdown content), SPEC-0024 (hover through the language server),
  SPEC-0072 (requests on unsaved text)
- **Feature matrix rows:** F-034

## Purpose
Read a symbol's signature and documentation comment without leaving the place you are editing.

## Behaviour
1. **Start:** `QuickJavaDoc` asks the active file's language server for the hover of the symbol at the
   caret. The server receives the editor's current text, so symbols that exist only in unsaved text are
   documented. Ferris IDEA asks for Markdown and accepts plain text. With no answer the status bar says "No
   documentation found."; a failure is shown there too. An answer that arrives after the caret moved or
   another file became active is dropped. [GEN, OSS, STD]
2. **Popup:** the documentation appears below the caret, at most 360 px high with scrolling, in blocks:
   headings, paragraphs (lines joined, inline Markdown markers and escapes removed), code in the editor
   font with its lines kept, and rules as separators. Rules at either end and repeated rules are dropped.
   [GEN]
   **Emphasis (v4):** single `*` emphasis markers are removed like `**` (jdtls ends its hover with
   `Source: *[project](file://…)*`, shown with the stars before, OBS-0016 Nr. 4); a `*` with a space
   on both sides stays. [OBS-0016]
   **Underscore emphasis (v9):** `_text_` and `__text__` lose their markers as `*` does, by the
   CommonMark rule for `_`: a marker opens only where no letter or digit stands before it and
   text follows it, and closes only where text stands before it and no letter or digit follows
   it, on the same line and outside inline code; a marker without its partner stays. So
   `snake_case`, a lone `_` and `__init__` in inline code keep their underscores. Before v9 the
   CSS server's sentence on browser support (`_Widely available across major browsers …_`) was
   shown with an underscore at either end beside the completion list (OBS-0087 W2). An image
   whose target is embedded data (`![Baseline icon](data:image/svg+xml;…)`) is left out with its
   text, since the popup draws no images. [STD: CommonMark emphasis; OBS-0087]
   **Character references (v10):** outside inline code and code blocks, a character reference
   stands for its character: the numeric forms `&#60;` and `&#x3C;` and the names `lt`, `gt`,
   `amp`, `quot`, `apos`, `nbsp`, `ndash`, `mdash`, `hellip`, `laquo`, `raquo`, `copy`, `reg`,
   `trade`, `times`, `middot`, `rarr` and `larr`. A `&` without a closing `;`, or with an unknown
   name, stays as written; inside backticks the reference is kept as written. Before v10 the CSS
   server's syntax line beside the completion list read `&lt;color&gt;` (OBS-0097 HC5); it now
   reads `<color>`. This applies everywhere these blocks are drawn: the popup, the panel beside
   the completion list and the other places that show a server's Markdown. [STD: CommonMark
   entity and numeric character references; OBS-0097]
   **Links and repeats (v2):** a Markdown link or image shows only its text (`[greeter.rb](file://…)`
   becomes `greeter.rb`); a second link to the same target (addresses compared with repeated `/`
   collapsed) is left out with the `|` or `,` before it, and a paragraph or code block already shown
   is not shown again, so ruby-lsp's hover for a class it indexed twice reads "Definitions:
   greeter.rb" and its comment once. [GEN]
   **Javadoc and inline code (v3):**
   - A link's target ends at the first `)` that closes it with balanced parentheses; when the target
     holds an opening parenthesis that is never closed (jdtls writes `jdt://…(Collection.class#728)`),
     it ends at the last `)` before whitespace or the end of the line. Either way only the link text
     is shown (`[spliterator()](jdt://…)` reads `spliterator()`), never the address. [GEN; OBS-0012]
   - A list item that holds only a bold label ending in `:` (`* **Returns:**`), followed by indented
     list items, becomes a labelled section: the label in grey in a left column, each item as a line
     in the right column. A value on the label's own line (`* **Since:** 1.8`) is the section's only
     item. Indented lines without a bullet continue the item before them. Abbreviated javadoc labels
     are written out: `Impl Spec` → "Implementation Requirements", `Impl Note` → "Implementation
     Note", `API Note` → "API Note", `See` → "See Also". [GEN (the standard javadoc labels); OBS-0012:
     Ferris IDEA showed the raw bullets]
   - Text between backticks in paragraphs is drawn in the accent colour on a faint background instead
     of losing its marking. [GEN; OBS-0012]
   **Signature, parameters and source (v5):** jdtls starts its hover with the declaration as a
   plain line (`int lab.inventory.Discount.apply(int cents)`), lists `@param` tags without a
   description as bare names and ends with `Source: *[navp_b75acabe](file://…)*`, the internal
   project name of the server with a generated id (OBS-0016 Nr. 7). Therefore:
   - When the first block is a one-line paragraph without inline code that reads as a
     declaration — only names, dots, brackets, generics, commas, `@`, `?`, `&` and spaces, not
     ending in `.`, `:`, `!` or `?` — it becomes a code block. When its last name before `(` is
     qualified, the qualifier (`lab.inventory.Discount`) is shown above the code as the owner
     (a grey line), and the code keeps the unqualified name. A declaration with parameters shows
     one parameter per line, indented by four spaces, with the closing `)` and whatever follows it
     on a line of its own: `int apply(` / `    int cents` / `)`. Commas inside `<>`, `()` and `[]`
     do not split. [OBS-0016, GEN]
   - In the sections "Parameters" and "Type Parameters" an item that is only a name (no
     description) is left out; a section left empty is not shown. [OBS-0016: the reference shows
     no parameter line without a description]
   - A last paragraph `Source: <name>` becomes a grey footer under a rule naming the module:
     `<name>` without a trailing `_` followed by 6 or more hex digits (`navp_b75acabe` → `navp`).
     The reference shows the module at the bottom of the popup. [OBS-0016]
   Other servers' hovers (clangd, rust-analyzer, pyright, gopls: a fenced or heading first block)
   are not changed by these rules. [GEN]
   The same blocks fill the documentation panel beside the completion list (SPEC-0072 v5). [GEN]
3. **Close:** Escape closes the popup and is not passed on; any other key closes it and acts as usual
   (typing goes on). The editor keeps the focus throughout. [GEN]
4. The MCP counterpart is `hover_symbol` (SPEC-0024); it returns the same Markdown text and works on the
   saved file. [ADR-0008]
5. **On mouse hover (v6):** when the pointer rests half a second on a word (letters, digits, `_`,
   `$`) of a file that has a language, in the text and not in the gutter, with no problem under it,
   Ferris IDEA asks the language server for the hover there, on the editor's current text (a remote
   document's from its host, SPEC-0318). The answer opens the same popup as behaviour 2, below the
   start of that word. Moving the pointer off the word closes it, unless the pointer moves into the
   popup; leaving the popup closes it. An answer that arrives after the pointer left the word, or
   after the text changed, is dropped. No answer shows nothing (no status message: the pointer only
   passed by). A popup opened by a key (F1, the problem popup by ⌘F1) stays until closed as before;
   a problem under the pointer shows the problem popup (SPEC-0078 v2 behaviour 5) instead. Keys
   close the hover popup as in behaviour 3. Before v6 Ferris IDEA showed documentation only by key, so
   resting the mouse on a symbol showed nothing (hand test Nr. 67, Vue). [GEN, DOC]
6. **Type Info (v7):** `ExpressionTypeInfo` (⌃⇧P in the bundled macOS keymap, Ctrl+Shift+P in
   the default one; View › Type Info, SPEC-0124 v14 behaviour 16) shows the type of the
   expression at the caret: one line of code in the popup of behaviour 2, below the caret,
   closed as behaviour 3 says.
   - **Where it asks.** Ferris IDEA asks the language server for the hover at the caret, with a
     selection at the selection's start, on the editor's current text (a remote document's
     from its host, SPEC-0318). An answer that arrives after another file became active or
     after the text changed is dropped.
   - **What it shows,** in this order: (a) the type of a `Type:` line of the hover (clangd);
     (b) the declared type of the word at the caret — the word the caret stands in or
     directly behind, or the selection when it is one word — where a line of the hover's
     code reads `name: Type` (`let mut total: Vec<u8>`, `(parameter) sku?: string`,
     `const LIMIT: usize = 4` without the value); a name inside a parameter list is not taken
     for it; (c) `var name Type` (gopls) and `Type name` (jdtls); (d) else the last code block
     of the hover as one line: the signature of a function, or the type alone, which is what
     a server answers for an expression. A hover without a code block gives its first line.
     More than 240 characters end with three dots.
   - **Nothing to show.** No answer, or an answer without text: the status bar says that
     there is no type information for the place, and no popup opens. A failed request says
     why in the status bar. In a file of no language, and without an editor, the action is
     not available.
   - The type is what the server's hover names, read from its text: LSP has no request for
     the type of an expression, so the result depends on the server, and for an expression
     that is no name on what the server answers at its first character. UI automation knows
     that the action changes nothing outside the window (SPEC-0319); for agents
     `hover_symbol` returns the hover itself (behaviour 4), there is no further tool.
   [OSS: the id, the title "Type Info" from `ActionsBundle.properties` and the chords of the
   bundled keymap files; STD: LSP `textDocument/hover`; GEN. How the reference shows the type,
   and that it widens the expression on a second press, was not observed and is not built.]
7. **External Documentation (v8):** `ExternalJavaDoc` (⇧F1 in the bundled keymaps) asks the
   language server for the documentation at the caret, as behaviour 1 does, and opens its first
   `https://` link in the system's browser; the status bar names the address. Without such a
   link, or without a language for the file, the status bar says "No external documentation
   link for the symbol at the caret". Links of other schemes (`http:`, `file:`, `jdt:`) are
   never opened. [OSS, GEN]

## Acceptance criteria
- **AK-1** Hover on unsaved text as Markdown with the signature and the documentation comment
  (integration test with clangd).
- **AK-2** Markdown to blocks: headings, paragraphs, code, rules, escapes, unclosed code (unit tests of
  `doc_model`).
- **AK-3** In a real window (`ferris-idea ui --selftest`, with clangd): F1 on a function's name shows its
  signature as code; Escape closes the popup and the caret stays.
- **AK-4** Use by hand: long documentation scrolls, other languages, links and lists.
- **AK-5 (v2)** Links, a repeated link target and a repeated paragraph (unit tests
  `doc_model::tests::links_show_their_text`, `doc_model::tests::repeated_documentation_is_shown_once`).
- **AK-6 (v3)** A jdtls link with an unbalanced `(` in its target shows its text; javadoc tag bullets
  become labelled sections with written-out labels; inline code keeps its place in the paragraph
  (unit tests `jdt_links_show_their_text`, `javadoc_tags_become_sections`, `inline_code_is_marked`).
- **AK-7 (v3)** Use by hand: F1 on `items.stream()` in a Java file shows "Returns", "Since" and
  "Implementation Requirements" as sections and no `jdt://` address.
- **AK-8 (v4)** Single-star emphasis is removed, a lone `*` between spaces stays (unit test
  `doc_model::tests::single_star_emphasis_is_removed`).
- **AK-9 (v5)** The jdtls hover of `Discount.apply` gives owner `lab.inventory.Discount`, code
  `int apply(` / `    int cents` / `)`, the javadoc sentence, no "Parameters" section and the footer
  `navp`; a sentence as first paragraph and a clangd hover stay as they were (unit tests
  `doc_model::tests::a_jdtls_signature_becomes_code`, `doc_model::tests::parameters_split_at_top_level_commas`,
  `doc_model::tests::sentences_stay_text`).

- **AK-10 (v6)** The word under the pointer: letters, digits, `_` and `$` belong to it, its start
  and end, nothing on spaces, punctuation or past the end (unit test
  `doc_model::tests::word_under_the_pointer`); the hover answer for the word is shown, a stale one
  dropped (unit test `doc_model::tests::hover_answers_for_the_word_still_under_the_pointer`).
- **AK-11 (v6)** Window self-test (not yet run in a window): in a C file with clangd, resting the
  pointer on `calc` opens the popup with its signature; moving the pointer to empty space beside
  the line closes it.
- **AK-12 (v7, behaviour 6)** The type read from hover texts in the shapes of rust-analyzer,
  the TypeScript server, pyright, gopls, jdtls and clangd: declared types, a function's
  signature, a function named like its parameter, the type alone, nothing for an empty hover,
  a long type cut (`doc_model::tests::type_info_reads_the_type_from_a_hover`; the texts are
  written after the servers' shapes, not recorded from running servers). In a window of the
  test harness: with the caret behind a name the answer shows the type as code below the
  caret and Escape closes it; a selection that is no single word shows the type the answer
  holds alone; no answer says so in the status bar and opens nothing; in a text file the
  action is not available (`seventh_package_keymap.rs::type_info_shows_the_type_at_the_caret`,
  which hands the answer in instead of starting a language server). The action is in View
  and Find Action with its key in both bundled keymaps
  (`menus::tests::seventh_package_items_are_in_their_menus`,
  `everywhere::tests::seventh_package_actions_have_titles`). With a running language server
  and on screen: not checked.
- **AK-13 (v8, behaviour 7)** Unit test
  `workbench_keymap_eleventh::tests::the_first_https_link_is_opened` (Markdown link, angle
  brackets, no other scheme); window test `eleventh_package_keymap.rs`:
  `find_usages_asks_for_the_scope` (the message without a link, macOS).

- **AK-14 (v9, behaviour 2)** Unit test `doc_model::tests::underscore_emphasis_is_removed`:
  `_Widely available (Baseline since 2015)_` loses both underscores, `__bold__` both pairs,
  `snake_case`, `a _ b` and an unclosed `_x` keep theirs, inline code keeps its underscores,
  and an embedded `data:` image is left out while an image with an address keeps its text.
- **AK-15 (v10, behaviour 2)** Unit test `doc_model::tests::character_references_are_decoded`:
  `&lt;color&gt;`, `&#60;`, `&#x3E;`, `&amp;` and `&mdash;` are decoded in text and headings, a
  lone `&`, an unknown name and `&lt` without `;` stay, inline code keeps `&lt;` as written.

## Out of scope
A setting to switch documentation on hover off or change its delay, the documentation tool window,
links that open other symbols, rendered
lists other than javadoc tag sections, tables, links in the owner line and buttons in the footer,
external documentation (⇧F1, `ExternalJavaDoc`: LSP's hover carries no address of a symbol's
documentation, and a link in the hover text leads to what the comment mentions, not to the
symbol's own page), quick definition.
v7: widening the expression on a second press of Type Info, the types of several carets, the
type of a selection that the server does not answer for at its first character.

## Known deviations from the reference
The reference renders its own documentation for its languages; Ferris IDEA shows what the language server
sends, so content depends on the server. Lists other than javadoc tag sections are shown as plain text, links as their text (not clickable,
also `jdt://` links). Inline code keeps the proportional font, marked by colour and background.
- **External Documentation (v8):** the reference opens the address of the library's
  documentation that is set up for the SDK or library. Ferris IDEA has no such setting and takes
  the first https link of the server's documentation; a server that gives no link (jdtls gives
  `jdt:` links) leads to the message.
