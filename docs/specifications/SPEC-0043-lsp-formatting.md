# SPEC-0043: Code formatting through the language server

- **Version:** 7 · **Date:** 2026-11-13 (v7: yaml-language-server formats from 1.19 on; v6: what the second comparison after v5 found, OBS-0036 FM5b and FM6b: in SQL a join stands under the table name of its `from` line and the column list of `create table` opens on a line of its own, behaviour 11; in Markdown a blank line separates setext headings, lists with different markers, tables and fenced code blocks from their neighbours, the delimiter row of a table is filled with dashes, and several spaces in a paragraph become one, behaviour 12; the Markdown formatter checks its own result); 2026-11-13 (v5: what the side-by-side run of Reformat Code found, OBS-0036 FM1 to FM7: the indentation nobody chose comes from Code Style per language, behaviour 1; IDECraft formats SQL and Markdown itself, behaviours 11 and 12; typescript-language-server formats with the indentation asked for, behaviour 13; options for the HTML and XML servers, behaviour 14; the differences that stay are named under "Known deviations"); 2026-11-13 (v4: the HTML, CSS and JSON servers are asked to format; a file nothing can format, a server that formats whole files only, a missing tool of the server and a server that asks or complains instead of formatting are said in words the user can act on; behaviours 8 to 10, headless run of every language for F-039); 2026-11-12 (v3: `format_file` uses the project's Prettier where Reformat Code does and names the formatter, SPEC-0123 v5, OBS-0028 T3; v2: `format_file` on a Python file uses the project's ruff or black, SPEC-0329; OBS-0034 L9)
- **Sources:** GEN (IDEs reformat a file or a selection according to style rules; F-039), STD (LSP
  `textDocument/formatting`, `textDocument/rangeFormatting`, `FormattingOptions` with `tabSize`,
  `insertSpaces`, `trimTrailingWhitespace`, `insertFinalNewline`), SPEC-0040 (edits, preview, apply);
  observed: `clangd` formats with clang-format's style and ignores `tabSize`; v4: headless run of
  2026-11-13 (`format_file` on one badly formatted file per language of SPEC-0042, with the
  servers installed on the development machine: JSON, CSS, SCSS, Less and HTML answered "the
  language server cannot format a file"; a shell script came back as already formatted; a Scala
  file failed with "method not found: window/showMessageRequest"; Markdown, Groovy, SQL and a
  text file failed with messages that name neither the server nor a way out; the window did
  nothing at all for a file of unknown type), STD (LSP `window/showMessage` with the message
  types 1 error to 4 log, `window/showMessageRequest` whose result is the chosen action or
  null, `documentFormattingProvider` and `documentRangeFormattingProvider` as boolean or
  options object), OSS (asked from the installed servers over the protocol:
  vscode-langservers-extracted 4.10 announces formatting and range formatting only with the
  initialization option `provideFormatter: true`; bash-language-server 5.6 announces formatting
  always, formats by running `shfmt` from the search path and answers without edits when there
  is none, logging a warning; metals 1.6.9 formats with scalafmt and, in a project without
  `.scalafmt.conf`, asks with `window/showMessageRequest` what to do and answers without edits;
  marksman and groovy-language-server announce no formatting; gopls, rust-analyzer, taplo,
  yaml-language-server (1.19 or later; up to 1.18 it announces formatting only through dynamic
  registration, which IDECraft does not offer, so it counts as having no formatter) and svelteserver
  announce formatting but no range formatting;
  vscode-css-language-server and vscode-html-language-server answer a formatted text with edits
  that replace it by itself), SPEC-0042 (the registry), SPEC-0123 (which files Prettier formats);
  v5: OBS-0036 (one badly formatted file per language reformatted by the reference with its
  defaults, no `.editorconfig`, no Prettier, next to `format_file`; findings FM1 to FM7), decision
  of the coordinator 2026-11-13 (formatters of IDECraft's own for SQL and Markdown, none for
  JavaScript, Groovy, Kotlin, CSS, HTML and XML: there only what a server option gives), OSS
  (asked from the installed servers over the protocol, and read in their open sources:
  typescript-language-server 6.0.2 asks the client for the section `formattingOptions` of a
  file when the file is opened, keeps the answer, and configures the TypeScript server with it —
  when that answer arrives after a formatting request, the file is formatted with the answer's
  indentation instead of the request's; the TypeScript server's format settings have switches for
  spaces and brace placement, none that breaks a block written on one line or puts the members
  of an interface on lines of their own; vscode-html-language-server reads `html.format` from
  `workspace/configuration`, where `extraLiners` names the tags that get a blank line before
  them, by default `head`, `body` and `/html`; lemminx takes `settings.xml.format` in its
  initialization options, where `spaceBeforeEmptyCloseTag` (default true) puts a space before
  `/>`; vscode-css-language-server 4.10 formats with the request's options alone and reads no
  `css.format`, `scss.format` or `less.format` settings; yaml-language-server normalises the
  spaces of a list written in one line and has no switch for it; kotlin-language-server formats
  with ktfmt and has no option that keeps arguments on several lines; groovy-language-server
  announces no formatting), STD (SQL: literals in single quotes with doubled quotes, quoted
  names, `--` and `/* */` comments, as in ISO/IEC 9075; the lexical differences of the
  databases as in SPEC-0005: backslash escapes and `#` comments in MySQL, nested comments and
  dollar-quoted text in PostgreSQL, `[name]` and `GO` in SQL Server), STD (CommonMark: ATX
  headings, fenced and indented code blocks, HTML blocks; GitHub Flavored Markdown: pipe tables
  with a delimiter row whose colons set the alignment, `\|` as a pipe inside a cell), GEN (SQL
  written with one clause per line and the conditions aligned under `where`), SPEC-0005 (the SQL
  lexer), SPEC-0137 (EditorConfig), SPEC-0150 (Code Style); v6: OBS-0036, section "Nachvergleich
  nach #635" (the same files formatted by IDECraft with v5 next to the reference's results;
  findings FM5b for `query.sql` and FM6b for `notes.md`), STD (CommonMark: setext headings, a
  list ends where the bullet character or the delimiter of the number changes, lazy
  continuation lines, code spans, thematic breaks, link reference definitions, hard line breaks
  written as two trailing spaces; GitHub Flavored Markdown: a table's delimiter row needs no
  spaces around its dashes)
- **Feature matrix rows:** F-039

## Purpose
Format code with the style rules of each language's own tooling (e.g. `clang-format` through `clangd`),
with a preview.

## Behaviour
1. A whole file or a range is formatted with the options tab size, spaces instead of tabs, trim
   trailing whitespace and insert a final newline (both default yes). The server's own
   configuration (e.g. `.clang-format`) takes precedence where the server applies it. [STD]
   **Indentation nobody chose (v5, OBS-0036 FM1):** before v5 `format_file` asked every server
   with four spaces, so JSON, YAML, SCSS and Less came out with four where the reference, in a
   project without `.editorconfig` and without formatter configuration, gives two. Now tab size
   and spaces-or-tabs come, in this order, from: the caller (`tab_size`, `insert_spaces` of
   `format_file`; the editor's indentation in the window), the file's `.editorconfig`
   (SPEC-0137), the user's Code Style for the file's language (SPEC-0150), and Code Style's
   built-in table — the one place the defaults are written down: two spaces for JSON, YAML,
   SCSS, Less and Ruby, tabs for Go, four spaces for CSS and every other language. The editor
   takes its indentation from the same table, so the window and `format_file` agree. [OBS,
   SPEC-0137, SPEC-0150]
2. A server without the matching capability is refused with a message saying so (whole file or range).
   [STD] (v4) The message names the server and what it lacks, behaviour 8.
3. The server's text edits become a plan as in SPEC-0040; the preview also gives the formatted text and
   whether the change is whitespace-only. A change that alters anything besides whitespace (e.g. sorted
   includes) is flagged, so the user can review it. [GEN, improvement IMP-0020]
4. Applying follows the SPEC-0040 rules (old text checked). No edits means the file is already formatted.
   [GEN] (v4) Edits that give the text the file already has count as none: some servers answer a
   formatted text with an edit that replaces it by itself, which before v4 was reported as a
   change and made an empty undo step. [OSS]
5. MCP tool `format_file` (path, optional range, options, `dry_run` default true). [ADR-0008]
6. **Python (v2):** a Python file is not sent to its language server (pyright and basedpyright
   have no formatter) but to the project's `ruff` or `black` (SPEC-0329); the answer has the same
   plan and names the formatter. [SPEC-0329]
7. **Prettier and the formatter's name (v3):** a whole file for which the project has Prettier
   is formatted with it in a trusted project, as Reformat Code does in the window (SPEC-0123
   v4 behaviour 5a); the answer has the same plan. Every answer of `format_file` names what
   formatted in `formatter` and `formatter_path`: for the language server its program name
   (e.g. `clangd`, `typescript-language-server`) and the program's path. [SPEC-0123, OBS-0028 T3]
8. **When nothing formats the file (v4):** the answer — the error of `format_file`, the status bar
   in the window (SPEC-0075 v3) — is one sentence that names who was asked, what is missing and
   what the user can do. Nothing is changed in any of these cases, and IDECraft formats nothing
   by rules of its own except SQL and Markdown (v5, behaviours 11 and 12). The cases, in the
   order they are checked:
   - **Unknown file type:** "No formatter for `<file name>`: IDECraft knows no language for this
     file type." No server is looked for.
   - **A language IDECraft serves itself** without a formatter of its own: "No formatter for
     `<language>` files: IDECraft serves them itself, without a language server, and has no
     formatter of its own for them." (v5) Before v5 this was the answer for SQL; SQL now has
     the formatter of behaviour 11, and no language is left in this case.
   - **No usable server:** the registry's message with the candidates and their install hints
     (SPEC-0042 behaviour 3), as for every other language feature.
   - **A tool the server formats with is missing**, checked before the server is started,
     because the server itself does not say so in its answer: bash-language-server formats with
     `shfmt` and answers without edits when `shfmt` is not on the search path — the message says
     that shfmt was not found and how to install it; metals formats with scalafmt and needs
     `.scalafmt.conf` in the project folder — the message says to create one and what it must
     set. IDECraft only looks whether the tool or file exists; it runs neither. Before v4 the
     shell script was reported as already formatted and the Scala file failed with a protocol
     error.
   - **The server has no formatter** (groovy-language-server; until v5 also marksman, whose
     Markdown files behaviour 12 now formats): "`<server>` has no formatter, so `<language>`
     files are not reformatted."
   - **The server formats whole files but no ranges**, and a range was asked for (gopls,
     rust-analyzer, taplo, yaml-language-server, svelteserver): "`<server>` formats whole files
     only, not a selection: reformat without a selection (MCP: without `range`)". The whole file
     is not formatted instead: that would change text outside the selection.

   Where Prettier formats files of that type (SPEC-0123 behaviour 1: Markdown, GraphQL, …) the
   first, second and fifth message add that the project's Prettier does it once `prettier` is in
   the package's devDependencies. [GEN, STD, OSS]
9. **What the server says instead of formatting (v4):** a server may show the user an error
   (`window/showMessage` with type 1) or ask a question (`window/showMessageRequest`) while it
   handles the request. IDECraft answers a question with no choice — it never picks one of the
   offered answers for the user — and keeps the texts. When the formatting request then comes
   back without edits, the answer is not "already formatted" but what the server said last:
   "`<server>` changed nothing and says: `<text>` [`<answer>` | `<answer>` …] (IDECraft chooses
   none of a server's answers for you)". Warnings, notes and log lines are not taken, and what a
   server said before the request does not count. Before v4 a question was rejected as an
   unknown method, which made the request fail. [STD]
10. **Servers that format only when asked to (v4):** vscode-html-language-server,
    vscode-css-language-server (CSS, SCSS, Less) and vscode-json-language-server get
    `provideFormatter: true` as initialization option, without which they announce no formatting.
    With it a whole file and a range are formatted with the options of behaviour 1. Where the
    project has Prettier, behaviour 7 still comes first for a whole file. [OSS]

11. **SQL (v5, OBS-0036 FM5):** the reference lays out an SQL file with its built-in formatter;
    IDECraft answered that it has none. IDECraft now formats SQL itself, on top of its SQL lexer
    (SPEC-0005), for a whole file; a selection is refused with "IDECraft's SQL formatter formats
    whole files only, not a selection: …" (how the reference formats a selection is not
    observed). No language server is asked; where the project has Prettier, nothing changes
    (Prettier formats no SQL). The answer of `format_file` names `idecraft` as formatter.
    - **Layout** of queries, `insert`, `update`, `delete`, `create table` and `create view`:
      every clause starts a line — `select`, `from`, `where`, `group by`, `having`, `order by`,
      `limit`, `offset`, `returning`, `values`, `set` of an `update`, `union`/`intersect`/
      `except`, and the `with` of a statement that began otherwise; a join starts a line under
      the table name of its `from` line, with its `on` condition on the same line (v6, see
      below); in `where`
      and `having` each `and`/`or` starts a line, right-aligned under the keyword so the
      conditions stand in one column (not the `and` of `between … and`, nothing inside
      `case … end` or parentheses); after `values` every row stands on a line of its own, the
      later rows under the first; the column list of `create table` opens with its parenthesis
      on a line of its own (v6, see below), has one
      entry per line indented by one level, the types of the columns start in one column after
      the longest name, and the closing parenthesis stands at the start of a line; a subquery or
      common table expression in parentheses is laid out the same way one level deeper, its `)`
      on a line of its own. Operators that stand between two values get one space on each side,
      a comma one space after it and none before, parentheses none inside; a sign stays at its
      value, a cast `::` and a `.` as close as written. Between two statements a blank line
      stays one blank line and none stays none. [OBS for the clauses, joins, `and`, operators,
      commas, rows and columns; GEN for the rest]
    - **Joins under the table name (v6, OBS-0036 FM5b):** v5 indented a join by one level; the
      reference indents it by nine spaces under `from shapes s`. A join line of any kind
      (`join`, `inner`/`left`/`right`/`full`/`cross`/`natural` … `join`) therefore starts one
      indentation level to the right of the column in which the table name of the `from` line
      starts: the indentation of that `from` line, plus the five columns of `from` and its
      space, plus one level — nine spaces at the outermost level with the level of four. What
      the observation does not show is decided so: the offset is counted from the `from`
      keyword of the same level, so the case in which `from` is written changes nothing, and in
      a subquery or common table expression the join stands the same nine columns to the right
      of that subquery's `from`; with another indentation the offset is five plus that
      indentation; a join that follows no `from` line of its level (`update t join …`,
      `delete from t join …`, where `from` starts no line) keeps the one level of v5. [OBS for
      the nine spaces; GEN for the rest]
    - **`create table` (v6, OBS-0036 FM5b):** v5 put the opening parenthesis of the column
      list after the table's name and a space; the reference puts it on a line of its own.
      After `create table <name>` the `(` now stands alone at the start of the next line, the
      entries follow indented by one level with the types aligned as before, and `)` with what
      follows it (`;`, table options) starts the line after the last entry. `create table … as
      select`, `create view` and every other statement stay as in v5: the observation shows
      nothing about them. [OBS]
    - **Whitespace only:** the formatter changes whitespace between tokens and nothing else.
      Keyword case stays as written (OBS-0036 does not say the reference changes it). Comments,
      string contents, quoted names and dollar-quoted text are never changed; a comment at the
      end of a line stays there, a comment on a line of its own keeps its line. No whitespace is
      added between two tokens that stood together, except around an operator between two
      values, after a comma, before the first row, and at a line break of
      the layout (the one before the column list included); none is removed between two tokens except before `,`, `;`, `)` and after `(`.
      Before a result is handed out it is read again: its tokens must equal the source's, and
      formatting it a second time must change nothing; otherwise the file is refused. The
      result is therefore idempotent. [GEN, security first]
    - **Statements without a layout** — everything else, e.g. `drop`, `alter`, `create index`,
      `grant`, `set`, a statement in parentheses, a statement with template braces or a psql
      backslash command — stay exactly as written, with their line breaks. Procedural code whose
      `;` end no statement (`create procedure|function|trigger|package|event` without a
      dollar-quoted body, `begin` that opens no transaction, `declare`, `delimiter`, `if`,
      `while`, `loop`) stays as written together with everything after it in the file. `GO`
      alone on a line stays on its line. [GEN]
    - **Refusal:** the file is left unchanged, with "IDECraft's SQL formatter changed nothing:
      line `<n>`: …", when the text cannot be read without doubt: a string, quoted name,
      comment or dollar-quoted text that never ends, parentheses that do not match, a comment
      that opens inside a comment (PostgreSQL nests them, others do not), or a backslash before
      a quote in a string (it ends the string in standard SQL and not in MySQL). The file is
      read without knowing its database: `#` to the end of the line (a comment in MySQL, an
      operator elsewhere) and `[…]` on one line (a name in SQL Server, an index elsewhere) are
      kept as they are. [STD, security first]
    - Indentation is the tab size of behaviour 1, always with spaces, because the aligned parts
      need them. Line breaks stay `\r\n` in a file that has only those.
12. **Markdown (v5, OBS-0036 FM6; v6, FM6b):** the reference puts blank lines between blocks,
    aligns the columns of tables and removes extra spaces from running text; IDECraft answered
    that marksman has no formatter. Without Prettier (behaviour 7 comes first) IDECraft formats
    a whole Markdown file itself, without any server; a selection is refused as in behaviour 11.
    v5 only put blank lines around ATX headings and aligned tables with spaces in the delimiter
    row. It does three things and nothing else:
    - **One blank line between blocks (v6):** where two blocks follow each other without a
      blank line, one is put between them
      - after a setext heading (text underlined with a line of `=` or of `-`),
      - before and after an ATX heading (`#` to `######` followed by a space or the end of the
        line, indented by at most three spaces),
      - before and after a pipe table,
      - before and after a fenced code block,
      - between two lists whose markers differ (`*`, `+`, `-`, a number with `.`, a number with
        `)`): CommonMark reads these as two lists.

      Never more than one is put, and none at the start or the end of the file. Where a blank
      line stands already nothing is added; several blank lines stay as many as they are (the
      observation shows no file with several). Nothing is inserted between the items of one
      list, between a list item and what belongs to it (lines indented to the item's text,
      nested lists whatever their marker), inside a block quote, a code block, the front
      matter or an HTML block, or between any other pair of blocks (paragraph and list,
      paragraph and quote, …). [OBS for the five places; GEN for the rest]
    - **Tables:** a pipe table — a header row, a delimiter row and rows that all start and end
      with `|` and have the same number of cells — gets every cell padded to the widest of its
      column, left, right or centred as the colons of the delimiter row say, with one space on
      each side. (v6) The delimiter row is filled with dashes without spaces over the whole
      width of the column, padding included (`|-----------|-------|`); an alignment colon takes
      the place of the first or the last dash (`|:-----|-----:|:---:|`). A column is as wide as
      its widest cell and at least one character (v5: three), so a delimiter cell has at least
      three characters. The text of a cell is not changed, runs of spaces inside it included.
      A line of text that follows the last row without a blank line is a paragraph, as in the
      reference: it gets the blank line before it (GitHub would read it as one more row).
      [OBS]
    - **Spaces in a paragraph (v6):** in a paragraph that stands at the top level of the file,
      every run of two or more spaces between two words becomes one space. Not changed are:
      the indentation of a line; spaces at the end of a line (two of them are a hard line
      break); tabs; the text of a code span (also one that runs over several lines of the
      paragraph); what stands between `<` and `>` on one line (an HTML tag, an autolink, a
      link target in angle brackets); the parentheses of a link or image after `](`; a line
      that starts a link reference definition (`[label]: …`). Not touched at all, because the
      observation shows only running text: the text of headings, list items, block quotes and
      table cells. [OBS for running text; STD and security first for the exceptions]
    - **Passed over:** fenced code blocks (one that never closes takes the rest of the file),
      lines indented by four or more spaces or a tab outside a paragraph, front matter between
      `---` lines at the top, HTML comments, `pre`/`script`/`style`/`textarea` blocks, other
      HTML blocks up to the next blank line, thematic breaks, block quotes with the lines that
      continue them, and list items with everything that belongs to them: lines indented to
      the item's text, and lines of text that continue an item or a quote without indentation
      (lazy continuation) — a line of `=` or `-` there is no heading and a row of pipes there
      is not formatted as a table. A table whose rows differ in length or lack the outer
      pipes is left as it is together with the lines up to the next blank line, and so is a
      line with a pipe that is underlined with dashes (it may be a table, not a setext
      heading). [STD, security first]
    - **Whitespace only, checked:** no character of the file changes except spaces, line
      breaks that make a blank line, and the dashes of a delimiter row; the final line break
      and `\r\n` stay. (v6) Before a result is handed out it is checked like the SQL
      formatter's: without whitespace and dashes it must equal the source, and formatting it a
      second time must change nothing. Otherwise the file is left unchanged with "IDECraft's
      Markdown formatter changed nothing: it would have changed more than whitespace here, so
      it left the file as it is (please report the file)". The result is therefore idempotent.
      [GEN, security first]
13. **typescript-language-server formats with the indentation asked for (v5, OBS-0036 FM3):**
    without Prettier a badly formatted JavaScript or TypeScript file came out with the
    function body indented by six where the reference gives four. Cause: the server asks for a
    file's `formattingOptions` when the file is opened; IDECraft answers that with the
    indentation the file already uses (SPEC-0095 v7 behaviour 3a, meant for refactorings), which
    for an unformatted file is whatever its first indented line has, and when that answer
    reaches the server after the formatting request it wins. While a formatting request for a
    file is under way — from before the file is opened until the answer — IDECraft now answers
    `formattingOptions` for that file with the request's tab size and spaces-or-tabs; SPEC-0095
    behaviour 3a holds at all other times. [OSS, OBS]
14. **Options for the HTML and XML servers (v5, OBS-0036 FM2):** vscode-html-language-server is
    told `html.format.extraLiners = ""`, so no blank line is put before `head`, `body` and
    `</html>`; lemminx is told `xml.format.spaceBeforeEmptyCloseTag = false`, so an empty
    element ends with `/>` without a space before it. Both are what the reference's result
    looks like. [OSS, OBS]

## Acceptance criteria
- **AK-1** Whitespace-only detection for unchanged, re-indented and reordered texts.
- **AK-2** With `clangd`: a badly indented C file is formatted (whitespace-only, compiles, idempotent: a
  second run has no edits); range formatting changes only the given lines; the project's `.clang-format`
  (indent width 4) takes precedence over the default style. (Skipped without `clangd`.)
- **AK-3** The MCP tool previews and applies formatting.
- **AK-4 (v3, behaviour 7)** The answer names the language server (`protocol.rs`:
  `code_insight_tools` with clangd, `format_file_names_the_language_server` with
  typescript-language-server); the Prettier path is SPEC-0123/AK-9.
- **AK-5 (v4, behaviour 8)** The sentences for an unknown file type, a GraphQL file (with
  the Prettier hint), a server without a formatter, a range with a server that formats whole
  files only, and the missing `shfmt` and `.scalafmt.conf`, which are gone once the tool or
  file exists (unit tests `format::tests::{files_without_a_formatter,
  formatting_a_server_does_not_offer, tools_a_server_formats_with}`). With the real servers
  (`crates/languages/tests/it/servers.rs::servers_that_cannot_format_say_why`, each part skipped
  without its server): groovy-language-server (v5; until v4 marksman), gopls with a range and
  with the whole file, bash-language-server without shfmt. (v5) SQL is no longer among the
  sentences.
- **AK-6 (v4, behaviour 9)** A server's question is answered with null and kept with its
  answers, an error it shows is kept, a warning and a log line are not, and only the newest
  twenty are kept (`lsp::tests::questions_and_errors_of_the_server_are_kept`); the sentence made
  of the last one (`format::tests::what_the_server_said_instead_of_formatting`). With a real
  server that asks (metals without `.scalafmt.conf` is caught by behaviour 8 first): open.
- **AK-7 (v4, behaviours 4 and 10)** Only the three servers get the option
  (`servers::tests::web_servers_are_asked_to_format`). With vscode-langservers-extracted
  (`crates/languages/tests/it/servers.rs::vscode_servers_format_files_and_ranges`, skipped
  without): a JSON, a CSS, an SCSS, a Less and an HTML file are laid out, whitespace only;
  formatting the result again has no edits; a range in a JSON file leaves the lines before it.
- **AK-8 (v4, behaviours 8 and 10)** MCP: `format_file` on a text file (until v4 also on an SQL
  file) is an error with the sentence and leaves the file; on a JSON file it names
  vscode-json-language-server and formats the file and a range
  (`crates/mcp/tests/it/protocol.rs::format_file_without_a_formatter_and_json`). `idecraft call`
  on one badly formatted file per language of SPEC-0042.

- **AK-8a (v5, behaviour 1)** The built-in table: two spaces for JSON, SCSS and Less, four for
  CSS (`ide_settings::tests::code_style_and_vcs_defaults`). The options of a file per language,
  the user's Code Style over the table, an `.editorconfig` over both
  (`format::tests::default_options_per_language`). With the real servers, in a folder without
  settings (`crates/languages/tests/it/servers.rs::formatting_with_the_defaults_per_language`,
  each part skipped without its server): a JSON, an SCSS, a Less and a YAML file come out with
  two spaces, a CSS file with four. MCP: `format_file` lays out a JSON file with two spaces, with
  four when `tab_size` says so, with three when the `.editorconfig` says so
  (`protocol.rs::format_file_without_a_formatter_and_json`).
- **AK-9 (v5, v6, behaviour 11, layout)** A query with joins, `where` with `and`/`or`, `group by`,
  `having`, `order by` and `limit`; kept keyword case, `between … and`, `case … end`, signs,
  casts and numbers with an exponent; `insert … values` with one row per line, also with
  `on conflict` and with common table expressions; `update` and `delete`; `create table` with
  aligned types and constraints, `create table … as`; subqueries, `union`
  (`sql_format::tests::{a_query_gets_one_clause_per_line, case_between_and_signs,
  insert_update_delete, create_table_aligns_the_types, subqueries}`; every expected text is
  formatted a second time without change and has the tokens of its source). (v6, FM5b) A join
  stands nine columns right of its `from`, whatever the case of the keyword, also in a subquery
  and a common table expression (nine columns right of that `from`), seven with the indentation
  of two, one level where no `from` line precedes it (`joins_stand_under_the_table_name`); the
  `(` of `create table` on its own line, also after a comment, with table options after `)`
  (`create_table_aligns_the_types`); the `query.sql` of OBS-0036 comes out as described there
  (`the_observed_file`).
- **AK-10 (v5, behaviour 11, safety)** Comments and string contents stay
  (`sql_format::tests::comments_and_strings_stay`); other statements, a function with a
  dollar-quoted body, a procedure and what follows it, `GO`, a transaction and template text stay
  as written (`other_statements_stay`); an open string, comment and dollar quote, parentheses
  that do not match, a backslash before a quote and a comment in a comment are refused with the
  line (`unreadable_text_is_refused`); `\r\n`, an empty file and the indentation option
  (`line_breaks_and_indent`); every `.sql` file under the repository's `tests/` folder keeps its
  token stream and is unchanged by a second run
  (`the_repository_s_sql_files_keep_their_tokens`). MCP
  (`protocol.rs::format_file_without_a_formatter_and_json`): preview with `formatter`
  `idecraft`, the same tokens before and after, `tab_size`, writing, no edits on the second
  call, a range refused, an unreadable file refused and left as it is.
- **AK-11 (v5, v6, behaviour 12)** Blank lines around ATX headings and what is no heading;
  tables with the three alignments, an escaped pipe, an empty cell, `\r\n` and indentation, the
  delimiter row filled with dashes, and the tables left alone; code blocks, front matter and HTML
  blocks passed over (`markdown_format::tests::{headings_get_blank_lines, tables_are_aligned,
  code_and_front_matter_stay}`). (v6, FM6b) A blank line after a setext heading, around tables
  and fenced code blocks and between lists with different markers, none added where one stands
  and none inside a list, a quote or their continuation lines (`blocks_are_separated`,
  `lists_and_quotes_stay`); runs of spaces in a paragraph with every exception of behaviour 12
  (`spaces_in_paragraphs`); the `notes.md` of OBS-0036 comes out as described there
  (`the_observed_file`). Every expected text is unchanged by a second run and differs from its
  source only in whitespace and dashes; a result that fails that check is refused
  (`an_unsafe_result_is_refused`), and every Markdown file under the repository's `docs/` folder
  is formatted without a refusal (`the_repository_s_markdown_files_keep_their_text`). A Markdown file is planned without a server, from the
  editor's text, and a selection is refused (`format::tests::own_formatters`,
  `servers.rs::servers_that_cannot_format_say_why`); MCP as AK-10.
- **AK-12 (v5, behaviour 13)** While a formatting request for a file is under way,
  `formattingOptions` is answered with its options, for that file only, and with the file's own
  indentation again afterwards (`lsp::tests::formatting_options_follow_the_file`). With
  typescript-language-server (`servers.rs::formatting_with_the_defaults_per_language`): a
  TypeScript and a JavaScript file whose function body is indented by six come out with four,
  three times in a row; without the change the test fails with six.
- **AK-13 (v5, behaviour 14)** The two options are sent
  (`servers::tests::html_and_xml_servers_get_their_format_options`). With the real servers: an
  HTML file has no blank lines around `body` (`servers.rs::vscode_servers_format_files_and_ranges`),
  an XML file closes an empty element with `"/>` (`formatting_with_the_defaults_per_language`).
- **AK-14 (v5)** Next to the reference again, per language of OBS-0036: done once after v5 (OBS-0036,
  "Nachvergleich nach #635"); after v6 for `query.sql` and `notes.md`: open.

## Out of scope
Format on save and on type (`onTypeFormatting`), IDECraft's own formatter for further languages without a
formatting server (Groovy among them; decision 2026-11-13: none for JavaScript, Groovy, Kotlin, CSS, HTML
and XML), formatting a selection of an SQL or Markdown file, a choice of SQL styles (keyword case, comma
placement, one column per line in `select`), the layout of procedural SQL, Markdown beyond the three
rules of behaviour 12 (list markers, numbering, heading style, line wrapping, emphasis marks), formatting the whole file when a selection was asked for and the
server formats whole files only, choosing an answer to a server's question, running `shfmt`, `scalafmt` or
another formatter directly instead of through the server, importing the reference's code style schemes.

## Known deviations from the reference
The reference formats with its own code style engine and settings; IDECraft uses each language's
standard formatter through its server, and does not copy the reference's style schemes. So the result
depends on what is installed: without a server, or with a server that has no formatter (Groovy)
or lacks its tool (shell scripts without shfmt, Scala without `.scalafmt.conf`), IDECraft
says what is missing (behaviour 8) where the reference formats with its built-in formatter.

Compared side by side per language on 2026-11-13 (OBS-0036). After v5 these differences stay, each
because no option of the server gives the reference's result and IDECraft writes no formatter of its
own for the language:
- **D1 JavaScript and TypeScript without Prettier (FM3):** a block written on one line stays on one
  line (`if (w < 0) { return 0 }`), an interface written on one line keeps its members on that line,
  and no blank line is put between a function and the code after it; the reference breaks the blocks,
  gives every member a line and adds the blank line. The TypeScript server's formatter has no option
  for any of it. The continuation lines of a list are indented by one level. With Prettier in the
  project its layout applies (behaviour 7).
- **D2 CSS, SCSS and Less (FM2, rest of FM1):** every selector of a rule stands on a line of its own
  where the reference keeps `.a, .b {` on one line; `(max-width:600px)` gets no space after the colon;
  a blank line separates rules, also after the first line of an SCSS or Less file.
  vscode-css-language-server 4.10 reads no format settings.
- **D3 HTML (rest of FM2):** every element stands on a line of its own, also a `head` that holds only a
  `title`; the reference keeps that on one line.
- **D4 YAML:** the spaces of a list written in one line are normalised (`[a, b, c]`); the reference
  leaves such a list as it is.
- **D5 Kotlin (FM4):** kotlin-language-server (ktfmt) joins call arguments that fit on one line and
  removes the blank line after `package`; the reference keeps both.
- **D6 Groovy (FM7):** groovy-language-server has no formatter, so a Groovy file is not reformatted;
  the reference formats it.
- **D7 TOML:** taplo joins a short list written over several lines; the reference keeps it.
- **D8 SQL (FM5, FM5b):** IDECraft's layout follows what OBS-0036 describes; details the observation
  does not give are IDECraft's own (the select list on one line, subqueries one level deeper, the
  place of a join in a subquery, with another indentation and without a `from` line, `create table
  … as` and `create view` as in v5). Statements without a layout and procedural code stay as written,
  and a file whose tokens depend on the database is refused (behaviour 11), where the reference
  formats with the dialect of the file.
- **D9 Markdown (FM6, FM6b):** only blank lines between the blocks named in behaviour 12, tables and
  the spaces of top-level paragraphs are touched. Several blank lines are not reduced to one, and
  list items, block quotes and headings keep their spaces; what the reference does there is not
  observed.
- **Deliberately different:** IDECraft formats a Dockerfile (the reference leaves it) and names the
  missing `shfmt` for a shell script (the reference leaves the script without a word).

Not compared: Go, Rust, C, C++, PHP, Ruby, Python, Scala, Swift, Lua, R, Vue, Svelte, and formatting a
selection.
