# SPEC-0089: Database tool window

- **Version:** 17 · **Date:** 2026-11-23 (v17: after a run that may change the schema the tree reads a
  connection it has read again, routines included, behaviour 29, AK-32–33 — OBS-0120 X2); 2026-11-19 (v16: the data sources of the reference's `.idea/dataSources.xml` that are not added yet are offered below the connections, added only through the connection dialog, behaviour 28, AK-31 — SPEC-0049 v7, SPEC-0008 v9, gap DS1 of OBS-0109); 2026-11-17 (v15: a foreign key's row and its documentation name its actions, an
  index's row and line its access method, behaviour 27, AK-30 — SPEC-0013 v8, OBS-0076 D1–D2); 2026-11-16 (v14: the selection stays on its node while rows come and go, also
  while a connection is read again after Copy Table, and goes to the nearest parent of a node that
  is gone, behaviour 26, AK-29 — screen run of 2026-11-16, B3); 2026-11-16 (v13: routines, triggers and types in folders of their kind,
  Show DDL on them, behaviour 25, AK-27–28 — SPEC-0013 v6/v7); 2026-11-16 (v12: once a connection is
  read, its row says whether the connection is encrypted, behaviour 24, AK-26 — SPEC-0016 v3,
  IMP-0421); 2026-11-16 (v11.1: AK-7 restated by what the user sees — a table
  created meanwhile loses its unknown mark after Refresh; its old wording, no cached schema right after
  Refresh, contradicts SPEC-0192 v3 behaviour 11, which reads the schema again at once for the open
  files; no change of behaviour, found by the Linux window self-test); 2026-11-15 (v11: every Refresh
  tells the SQL editors, also the toolbar button, behaviour 23 — OBS-0051 L7, re-check of PR #780); 2026-11-14 (v10.1: a row chosen while a connection is read again stays chosen, behaviour 22, from the window self-test of 15:15); 2026-11-14 (v10: Refresh and saving a connection keep the open nodes and the selection, behaviour 22); 2026-11-13 (v9: the condition of a partial index and the collation of an
  index term in the tree and the documentation, a view that cannot be read says why and has no arrow
  to expand — from the window check with SQLite files; v8: `describe_table` and `table_ddl` take a schema, such as an
  attached SQLite database — notes from the SQLite check in PR 650; v7: on SQLite Show DDL gives the statements the database
  stores, with indexes and triggers; generated columns in the tree and the documentation — from the check
  of the catalogue sample without a screen; v6 2026-11-07: quick documentation, toolbar, colour, schema filter —
  OBS-0026 L30, L32; v2: look from hand test 16; v3: all schemas and an error node from
  OBS-0010, IMP-0051, IMP-0052; v4: Refresh also refreshes the SQL editors, the loading row of a connection with a keychain
  password says the system may ask first — hand tests 78 and 55, D31, D21; v5: speed search, Show DDL and the
  sequences of a PostgreSQL schema from the side-by-side comparison OBS-0026)
- **Sources:** GEN (a database tool window at the right edge lists the project's data sources; expanding
  one connects in the background and shows its tables and views with columns, keys and indexes; a
  refresh button reads them again; F-051, F-052), OSS (platform icons `expui/nodes/dataSchema`,
  `dataTables`, `dataColumn`, `status/warning`; tool window action naming `Activate<Id>ToolWindow`), SPEC-0008 (connection
  profiles), SPEC-0013 (introspection), RESEARCH_BOUNDARIES 5a (proprietary area: own texts, open icons), OBS-0010 (a data source whose
  introspection failed expanded to an empty tree; the cause stood only in its properties dialog; a newly
  created schema stayed hidden until ticked in the data source settings), IMPROVEMENTS IMP-0051, IMP-0052,
  OBS-0026 (v5: typing in the tree searched its names; a toolbar button and a menu entry showed a table's
  definition as SQL in an editor tab; a PostgreSQL schema listed its sequences with a count), STD (SQL
  `CREATE TABLE`/`CREATE INDEX`/`CREATE VIEW`, `information_schema.sequences`)
- **Feature matrix rows:** F-051, F-052

## Purpose
See what is in the project's databases without leaving the editor.

## Behaviour
1. **Open:** the Database button at the top of the right stripe, or `ActivateDatabaseToolWindow` (no
   default shortcut; found through Search Everywhere), shows the tool window at the right; pressed again,
   Escape in the tree or ⇧Esc hides it. [GEN, OSS]
2. **Connections:** one row per profile of `.ferris-idea/connections.toml`, with the dialect and where it
   points ("SQLite · shop.db", "PostgreSQL · host:5432/db"). Without profiles the window says where to add
   them; a file with problems names the problem. ↑/↓ choose, →/Enter expand, ← collapse, a double-click
   toggles. [GEN]
3. **Schema:** expanding a connection reads its current schema in the background ("Loading…"), with the
   password from the keychain entry of the profile. Failures (missing password, server not reachable)
   show as a grey row under the connection; expanding it again retries. [GEN]
4. **Tree:** "tables" and "views" with their counts; each table with "columns" (type in lower case,
   "primary key", "not null", default), "keys" (primary key and foreign keys "(cols) → table(cols)")
   and "indexes" ("(cols)", "unique"). [GEN]
5. **Refresh:** the refresh button reads the profiles again and forgets loaded schemas. [GEN]
6. Agents use `list_tables` and `describe_table` (SPEC-0013). [ADR-0008]
7. **v2 – Look:** the window is 400 px wide; names and details that do not fit end in "…"; details stay
   readable on the selected row. A failure row has the warning icon and the error without the repeated
   connection name ("Password missing in keychain entry …"), wrapping over several lines. Primary keys,
   foreign keys and indexes have Ferris IDEA's own icons (a yellow key, a blue key with an arrow, green lines
   with an arrow), since the reference's database icons are not open. [GEN, OSS]

8. **v3 – All schemas (IMP-0052):** under a connection, after the "tables" and "views" of its current
   schema, every other non-system schema of the database is a row with the schema icon ("audit",
   detail "schema"). Expanding one reads its tables and views in the background and lists them like
   behaviour 4 (columns, keys, indexes). System schemas (as in SPEC-0014 v2 behaviour 10) are not listed.
   SQLite lists attached databases other than `main` and `temp`; Oracle lists only the user's own schema.
   The refresh button and expanding the connection again after a refresh read the list anew, so a schema
   created by SQL appears without changing any setting. **v4:** Refresh (of the selected connection, or of
   all) also drops what the SQL editors cached about that schema (completion, inspections,
   refactorings), so a table created meanwhile is known in the editor at once, without a restart. [GEN]
   While a connection whose password is in the keychain is read, its loading row says that the system
   may ask for keychain access first, so a pending system prompt is not taken for a hang. [GEN]
9. **v3 – Error node (IMP-0051):** when reading a schema fails, the connection (or schema) expands to an
   error row "Could not read the schema: <cause>" with the warning icon, and under it a "Retry" row; Enter
   or a double-click on either reads it again. Whatever Ferris IDEA cached about that connection's schema
   (completion, inspections) is dropped, so a retry starts clean; Ferris IDEA keeps metadata only in memory,
   so no restart is ever needed. `list_tables` reports the same cause over MCP; `list_schemas` (connection)
   lists the schemas with the current one marked. [GEN, ADR-0008]
10. **v5 – Speed search:** a letter typed while the tree has the focus opens a small search field
    above the tree with the typed text and selects the first visible row whose name contains it,
    ignoring case, starting at the selected row (the selection stays while it still matches). The
    matched part of every visible name is highlighted. ↑/↓ go to the previous and next match (wrapping
    around), Backspace removes the last letter (the last one closes the field), Escape closes the
    field without hiding the window; any other key closes it and acts as usual (Enter expands the
    found row). Only visible rows are searched (collapsed folders are not opened); loading and
    error rows are skipped. Without a match the text shows in the error colour and the selection
    stays. The field has a tooltip that says how it is used. [OBS-0026, GEN]
11. **v5 – Show DDL:** the toolbar button "DDL" (after Refresh), the context menu entry "Show DDL"
    (enabled on tables and views) and ⌥⇧⌘D (Ctrl+Alt+Shift+D; free in both bundled keymaps — the
    reference's Edit Source key already opens the data or properties here) open the selected table's or
    view's definition as SQL in a read-only editor tab "<name> (DDL).sql", read anew each time from
    the connection. It is generated per dialect from Ferris IDEA's own introspection (SPEC-0013, the same
    generator as SPEC-0018): `CREATE TABLE` with the columns (type, `NOT NULL`, default), the primary
    key and the foreign keys (`REFERENCES`), then the table's own indexes (not the primary key's or,
    on MySQL, a foreign key's implicit index). A view is `CREATE VIEW <name> AS <query>` with its
    stored query; where the query cannot be read a comment says that the definition is not available.
    Agents use the MCP tool `table_ddl` (connection, table), which returns name, kind and the same
    text. [OBS-0026, STD, GEN, ADR-0008]
12. **v5 – Sequences:** under a PostgreSQL connection, after "tables" and "views", a "sequences" row with
    their count lists the sequences of the current schema (e.g. the one of a `serial` column), read
    with the schema from `information_schema.sequences`. Other dialects and the other schemas of
    behaviour 8 have no sequences row. [OBS-0026, STD]
13. **v6 – Quick documentation:** F1 or ⌃J (and the toolbar's "?") shows, above the tree, what the tree
    already read about the selection: for a table or view its kind and name, connection and database,
    each column with type, `PK`, `→ table(column)` for a foreign key, `NOT NULL` and default, the primary
    key, the foreign keys, the tables that reference it and the indexes; for a column its type, default,
    keys and indexes; for a connection its system and URL (never a password), tunnel, SSL mode,
    read-only, commit mode, time limit, schema filter and comment. It follows the selection while open;
    F1, ⌃J or Escape hide it. Nothing is asked of the database and nothing waits for a language engine
    (IMP-0367); a table that is not read yet says to expand it. MCP `object_doc` (connection, table,
    optional column) returns the same lines. [OBS-0026, ADR-0008]
14. **v6 – Toolbar:** after +, ✎, Refresh and DDL: "SQL" opens a new console of the selected connection
    (`Console.Open`), the table button opens the selected table's data (as F4), "?" the quick
    documentation, "▽" switches the schema filter (behaviour 16) and Collapse All closes every row and
    keeps the selection on its connection. Each has a tooltip with its shortcut. [OBS-0026, GEN]
15. **v6 – Colour:** a connection with a colour (SPEC-0108 v4 behaviour 9) shows a dot of it next to
    its icon. [GEN]
16. **v6 – Schema filter:** the other schemas of a connection (behaviour 8) are those its schema filter
    allows (SPEC-0108 v4 behaviour 10; empty: all). "▽" in the toolbar shows all schemas of every
    connection instead, and again only the filtered ones; each switch reads the lists again. [GEN,
    OBS-0010]

17. **v7 – Show DDL on SQLite:** for a SQLite connection the definition (behaviour 11, also MCP
    `table_ddl`) is not generated but read: the statement the database stores for the table or view in
    `sqlite_master`, as it was written, then, after an empty line, the stored statements of its indexes
    and of its triggers, each sorted by name and ending in `;`. So the text has what the catalogue
    queries of SPEC-0013 cannot report — check constraints, generated columns and their expressions,
    collations, the actions and deferral of foreign keys, `AUTOINCREMENT`, `WITHOUT ROWID`, `STRICT`,
    `CREATE VIRTUAL TABLE` with its module arguments, partial and expression indexes — and run against
    an empty database it creates an equivalent object. Indexes SQLite made itself for a `UNIQUE` or
    `PRIMARY KEY` constraint have no statement and are part of the table's. A view whose tables are gone
    still shows its statement. The other dialects generate the text as before. [STD: SQLite schema
    table, column `sql`]
18. **v7 – Generated columns:** in the tree a generated column's detail ends in "generated (virtual)"
    or "generated (stored)" (SPEC-0013 v3 behaviour 8); the quick documentation and `object_doc` say
    the same on the column's line. [GEN]
19. **v8 – Objects of another schema over MCP:** `describe_table` and `table_ddl` take an optional
    `schema`, with the meaning `list_tables` gives it: a schema from `list_schemas`, on SQLite an
    attached database (`ATTACH … AS archive`, for instance from the connection's startup script).
    Without it the current schema is meant, as before. The structure is read from that schema's
    catalogue (SPEC-0013), the SQLite DDL from that database's `sqlite_master`, the generated DDL of the
    other dialects from that schema's tables and views. An object missing there is an error that names
    the schema. [GEN]

20. **v9 – Partial indexes and collations:** the detail of an index row is its terms in brackets,
    then, each after " · " like "unique" before, what else holds for it, in this order: "unique",
    "where <condition>" for a partial index (SPEC-0013 v4 behaviour 12). A term with a collation
    (SPEC-0013 v4 behaviour 13) reads "<column> collate <name>", the name in lower case like the
    types. Examples: "(city) · where deleted = 0", "(city collate nocase, name)",
    "(sku) · unique · where deleted = 0". A condition written over several lines is shown on one
    (runs of white space as one blank); the structure keeps it as written. The quick documentation
    and `object_doc` add the same to an index line ("Index indexed_active (city) where deleted = 0"),
    and the schema comparison names an index with them. [GEN]
21. **v9 – A view that cannot be read:** such a view (SPEC-0013 v4 behaviour 14) has the detail
    "cannot be read: <the database's message>" in the grey of the other details (after "view · "
    where the row already says so, that is under another schema), so the reason is visible without a
    click. A table or view row has the arrow to expand only when it has something below it —
    columns, keys or indexes; without them a click, → or Enter selects the row and opens nothing, and
    a row that was expanded before a refresh shows no open arrow. The quick documentation and
    `object_doc` say "Cannot be read: <message>" after the heading. Show DDL still shows the stored
    statement (behaviour 17). The rest of the schema is listed as usual. [GEN]
22. **v10 – Refresh keeps the tree (window check 2026-11-14):** Refresh in the toolbar, Refresh
    with no connection selected, saving a connection in its dialog and removing one read the
    connection file again without collapsing the tree: the open nodes and the selected row stay,
    found by the connection's id, so they follow a connection that moved in the list; an open
    connection or schema is read again and its open tables and sections show once it is loaded.
    Nodes of a connection that is gone go with it. Until v10 these collapsed the whole tree and
    selected the first row. v10.1: the selection is a node, not a row number: a row chosen while
    a connection is still being read stays chosen when the read rows appear above it, and the kept
    row is selected again only if no row was chosen meanwhile (until v10.1 the second Delete on a
    connection just saved could ask about the connection above it). [GEN]
23. **v11 – Every Refresh reaches the SQL editors (OBS-0051 L7, re-check of PR #780):** the Refresh
    button in the toolbar, Refresh in the context menu and the keymap, the schema filter and saving or
    removing a connection all read the connection file again through one path, which also tells the
    SQL editors that the schemas are out of date (behaviour 5 v4, SPEC-0192 v3 behaviour 11). Until
    v11 the toolbar button read the tree again without telling them: the tree showed a table created
    meanwhile, while an open `.sql` file kept marking it unknown, also after typing. [GEN]
24. **v12 – Encryption on the row (SPEC-0016 v3, IMP-0421):** once a connection's schema has been
    read, the detail of its row ends in "· TLS" when the session is encrypted and in "· TLS: no" when
    it is not, so a plain connection to a server on the loopback (`prefer` without TLS, or `disable`)
    is visible without a Test Connection. Before the read, after a failed read, for SQLite and DuckDB
    and when the server does not say, nothing is added. Refresh forgets it with the schema. [GEN]
25. **v13 – Routines, triggers and types (SPEC-0013 v6 behaviour 18, v7 behaviour 20):** when a
    connection's current schema is read, its routines, triggers and types are read with it (a
    failing read leaves them out, as the sequences of v5 behaviour 12). After the sequences each
    kind the connection has gets a folder with its count — `functions`, `procedures`,
    `aggregates`, `triggers`, `types`, in the order of SPEC-0013 v6 behaviour 18a; a kind without
    objects has none. A routine's row shows its name with its arguments on PostgreSQL, so overloads
    are told apart; a trigger's row its table and when it fires (`stock · BEFORE INSERT`); a
    type's row what it is (`enum`, `domain`, `range`, `composite`). Show DDL (context menu, Find
    Action) on such a row reads the definition of SPEC-0013 v7 behaviour 20 in the background and
    opens it in a read-only tab `<name> (DDL).sql` (a routine's name with its arguments); a
    missing privilege or a failing read goes to the status line. Rename is not offered on them.
    The other schemas of v3 behaviour 8 list only tables and views. [GEN]
26. **v14 – The selection is a node (screen run 2026-11-16, B3):** whenever rows come or go —
    a connection or schema is read (again), a node is expanded or collapsed, a store level arrives,
    Refresh — the selection stays on the node it was on, not on the row number it had. While the
    node has no row (its connection is being read again, as after Copy Table, SPEC-0191 behaviour
    6), its nearest parent that has one is selected, and the node is selected again once its row is
    back, unless the user selected another row meanwhile. A node that is gone after the read leaves
    the selection on that parent (a dropped table: the `tables` folder of its connection); a node
    hidden by collapsing a parent gives the selection to that parent for good. Until v14 the row
    number was kept while a connection was read again: after Copy Table on a table of one
    connection, the selection jumped to the row of another connection. [GEN]
27. **v15 – Actions and methods (SPEC-0013 v8 behaviours 21–22, OBS-0076):** the detail of a
    foreign key's row ends in its actions in small letters (`(a) → parent(id) on delete cascade on
    update set null`), and its line in the quick documentation in capitals (`… → parent(id) ON DELETE
    CASCADE`); nothing is added for `NO ACTION`. An index with an access method other than the
    default one ends its detail in `· gin` and its documentation line names it (`Index tags_idx using
    gin (tags)`). The condition of a PostgreSQL partial index shows like SQLite's (behaviour 20). [GEN]

28. **v16 – Data sources from `.idea` (SPEC-0049 v7 behaviour 18):** below the connections and stores the
    window lists, under the heading "From the project's .idea, not added yet", the data sources of the
    project's `.idea/dataSources.xml` that are not added yet (SPEC-0008 v9 behaviour 19): name, kind and file
    or server, all from the clean URL. Each offered one has an Add… button; data sources that cannot be offered
    are listed with their reason and no button. Add… opens the connection dialog filled with the profile of
    SPEC-0008 v9 behaviour 19 and a note saying where it came from; nothing is saved before the user saves in
    the dialog, nothing connects before Test Connection or the first use, and closing the dialog leaves the
    data source offered. Once saved the data source leaves the list. The list is read with the window and on
    every reload (Refresh, saving a connection); without data sources there is no heading. [GEN, CLAUDE.md
    rule 12]

29. **v17 – After a script (OBS-0120 X2):** when a run from an editor or console may have changed a
    connection's schema (the statements SPEC-0192 v5 behaviour 11a names: DDL, `CALL`, `EXEC`,
    blocks), the tree reads that connection again once the run has ended, as Refresh does (behaviour
    22): tables, views, sequences, procedures, functions and triggers alike, keeping the open nodes and
    the selection. A procedure a MariaDB script created between `DELIMITER` lines appears under
    Procedures without Refresh. A connection the tree has not read yet stays as it is. [GEN]

## Acceptance criteria
- **AK-1** Connection rows and details, loading, failure and empty states, groups, sections and the texts
  of columns, keys and indexes (unit tests of `db_tree_model`).
- **AK-2** In a real window (`ferris-idea ui --selftest`): a SQLite connection of the workspace is listed,
  expanding it lists two tables, the columns, the foreign key and the index of `orders` are shown, Escape
  hides the window.
- **AK-3** Use by hand with the reference databases (PostgreSQL, MySQL) next to the reference.
- **AK-4 (v2)** Short error texts (unit test `db_tree_model`); look by hand (hand test 16).
- **AK-5 (v3)** Rows for other schemas (not loaded, loading, loaded, failed) and the error and retry rows
  (unit tests of `db_tree_model`); schema lists without system schemas, live against PostgreSQL and MySQL:
  a schema created by SQL is listed after a fresh read and its table is introspected (integration tests).
- **AK-6 (v3)** In a real window: a connection whose file is missing expands to the error and retry rows;
  Enter on Retry reads it again (`--selftest`). `list_schemas` over MCP (protocol test).
- **AK-7 (v4, restated in v11.1)** Window self-test: an open `.sql` file of the selected connection
  marks a table that does not exist as unknown; the table is created outside Ferris IDEA and stays
  unknown until Refresh on the selected connection; after it the mark goes without an edit, and the
  SQL editors hold the schema read anew (`db_compare_steps`). Until v11.1 the criterion asked that no
  schema be cached right after Refresh; since SPEC-0192 v3 behaviour 11 the open files are inspected
  again at once and fill the cache with the new schema, so that state lasted only until the read
  ended and a check of it raced the read (it failed on Linux, where the read ended first).
- **AK-8 (v5)** Speed search matching, first match from the selection, ↑/↓ between visible matches, message
  rows skipped (unit tests `speed_search::tests::{matching_ignores_case_and_marks_the_part,
  selection_moves_between_visible_matches, what_types_into_the_search}`,
  `db_tree_model::tests::speed_search_finds_visible_rows`).
- **AK-9 (v5)** DDL of a table (columns, NOT NULL, defaults, primary key, foreign keys, indexes) and of a
  view per dialect (unit tests `ddl::tests::{object_ddl_of_a_postgres_table, object_ddl_of_a_mysql_table,
  object_ddl_of_a_sqlite_table_and_views}`).
- **AK-10 (v5)** `table_ddl` over MCP against SQLite (protocol test `table_ddl_of_a_table_and_a_view`); the
  context menu entry on tables only (`context_menu::tests::database_menu_states_follow_the_node`); the
  shortcut in both keymaps (`keymap::tests`).
- **AK-11 (v5)** The sequences row with its count (`db_tree_model::tests::sequences_group_with_count`);
  sequences read from PostgreSQL, none for SQLite (`crates/database/tests/it/catalog.rs`
  `sequences_of_a_schema`, PostgreSQL part with `FERRIS_IDEA_TEST_PG`).
- **AK-12 (v5)** Window self-test: typing "ord" selects `orders`, Escape closes the search only, Show DDL
  opens `orders (DDL).sql` with its `CREATE TABLE` (`db_search_ddl_steps`).
- **AK-13 (v6)** The documentation lines of a table (columns with keys and defaults, foreign key,
  referencing tables, indexes) and of a column; unknown names give none
  (`object_doc::tests::documents_tables_and_columns`); schema patterns (`profile::tests::
  schema_filter_and_colours`).
- **AK-14 (v6)** MCP `object_doc` for a table, a column and an unknown table (protocol test
  `grid_values_object_doc_history_and_connection_extras`).
- **AK-15 (v6)** Window self-test (`db_search_ddl_steps`, written, not run yet): F1 on `orders` shows
  `table orders` with its foreign key at once, F1 again hides it.
- **AK-16 (v6)** In the window next to the reference: the toolbar, Collapse All, the schema filter with
  PostgreSQL, the colour dot (L30, L32).

- **AK-17 (v7)** For every table and view of the catalogue sample the shown DDL, run into an empty
  database, gives the same columns (with hidden and generated ones), foreign keys with their actions,
  indexes with uniqueness, origin, partial flag, sort order and collation, table kind (`WITHOUT ROWID`,
  `STRICT`, virtual) and triggers as the original, compared through SQLite's pragmas; the text keeps
  the named check, the deferred key, `autoincrement`, the triggers of `ticket` and `CREATE VIRTUAL
  TABLE … fts5`; an unknown name gives nothing (`crates/database/tests/it/sqlite_catalogue.rs`
  `the_shown_ddl_recreates_an_equivalent_object`). Over MCP the stored statement
  (`protocol.rs` `table_ddl_of_a_table_and_a_view`).
- **AK-18 (v7)** The detail of a generated column in the tree (`db_tree_model::tests`) and its line in
  the documentation (`object_doc::tests::documents_tables_and_columns`).
- **AK-19 (v8)** Against a real SQLite file with an attached database: its table is described with its
  index and shown as stored DDL with its index, its view as a view; the same name without the schema
  gives nothing (`sqlite_catalogue.rs` `triggers_attached_objects_and_hidden_columns`).
- **AK-20 (v8)** Over MCP with an attached database from the startup script: `list_tables`,
  `describe_table` and `table_ddl` with `schema` find the table; without the schema or with `main` it is
  an error naming the schema (`protocol.rs` `describe_table_and_table_ddl_of_an_attached_database`).

- **AK-21 (v9)** The details of a partial index, a unique partial index, a term with a collation and a
  condition over several lines (`db_tree_model::tests::index_rows_show_condition_and_collation`); the
  index line of the documentation (`object_doc::tests::documents_tables_and_columns`).
- **AK-22 (v9)** A view that cannot be read: its detail with the message, no arrow and no open state
  even when its node is in the expanded set, under the current and under another schema; the sound
  view next to it keeps its arrow and its sections (`db_tree_model::tests::
  a_view_that_cannot_be_read_says_why_and_does_not_expand`); its documentation line
  (`object_doc::tests::a_view_that_cannot_be_read_says_so`). Over MCP: SPEC-0013 v4 AK-11.
- **AK-23 (v9)** In a real window with a SQLite file: the rows of `indexed_active` and of a broken
  view as above (open: by hand or self-test; not run when v9 was written).
- **AK-24 (v10, behaviour 22)** `db_tree_model::tests::open_nodes_keep_to_their_connection`: an
  open node keeps to its connection by id after the list changed order; a removed connection's
  nodes go. By hand: open a table's columns, Refresh — the columns stay open and selected.
  v10.1: `tests/it/database_selection_after_reload.rs`: a row chosen before the read of a
  connection above it finished stays chosen; without a choice the kept table row comes back.
- **AK-25 (v11, behaviour 23)** `tests/it/sql_refresh_from_toolbar.rs` (macOS, SQLite): an open
  `.sql` file names a table that does not exist and is marked unknown; the table is created; the
  toolbar's Refresh (`DatabaseView::refresh_from_toolbar`, what the button runs) clears the mark
  without an edit, and an edit afterwards does not bring it back.
- **AK-26 (v12, behaviour 24)** `db_tree_model::tests::a_read_connection_says_whether_it_is_encrypted`:
  "· TLS" and "· TLS: no" after the read, nothing before it or when unknown.
- **AK-27 (v13, behaviour 25)** `db_tree_model::tests::routines_triggers_and_types_in_folders`:
  folders in the dialect's order with their counts, overloads with their arguments, a trigger
  with table and timing, a type with what it is.
- **AK-28 (v13, behaviour 25)** Window test in the test harness
  (`tests/it/db_object_definitions.rs`: `show_ddl_opens_the_definition_of_a_trigger`, macOS;
  written, not yet built): a SQLite file with a trigger lists it under `triggers`, and Show DDL
  opens `stock_check (DDL).sql` read-only with SQLite's statement.
- **AK-29 (v14, behaviour 26)** Window test in the test harness (`tests/it/sql_beta_handtests.rs`,
  `b2_b3_copy_table_names_its_connection_and_keeps_the_selection`, macOS, SQLite, two connections):
  after Copy Table on `item` of the first connection the selection is still on `item` once the
  connection is read again; a selected table dropped outside the IDE leaves the selection on its
  `tables` folder after the connection is read again.

- **AK-30 (v15, behaviour 27)** `db_tree_model::tests::foreign_key_actions_and_index_methods_in_rows`;
  `object_doc::tests::foreign_key_actions_and_index_methods_in_the_documentation`.
- **AK-31 (v16, behaviour 28)** `crates/workbench/tests/it/idea_data_sources.rs`:
  `idea_data_sources_are_offered_until_added`: a workspace with `.idea/dataSources.xml` for a SQLite file and
  an unsupported driver: the Database window offers the SQLite one and lists the other with its reason;
  `connections.toml` does not exist; Add… opens the dialog with name, SQLite and the file; Save writes the
  profile, after which the data source is no longer offered.
- **AK-32 (v17, behaviour 29)** `crates/workbench/tests/it/db_object_definitions.rs`
  `objects_a_script_creates_appear_in_the_tree`: with the trigger folder of a SQLite connection open,
  running `CREATE TRIGGER` from a `.sql` file lists the new trigger beside the old one, without
  Refresh.
- **AK-33 (v17, behaviour 29)** Live (`crates/database/tests/it/dialect_numbers_live.rs`
  `routines_a_script_creates_are_listed`): a procedure and a function created by a script — MariaDB
  and MySQL between `DELIMITER` lines, SQL Server in `GO` batches, Oracle ended with `/`, PostgreSQL —
  are in the catalog the tree reads.

## Out of scope
Adding or editing connections in a dialog (F-051 remains partly open), renaming or comparing objects of
other schemas than the current one, routines and triggers, sequences of other schemas than the current one,
the DDL of objects of other schemas, editing the DDL tab (it is read-only), opening a table's data (F-054), a SQL console (F-053), drag and drop.

## Known deviations from the reference
The reference shows many more object kinds and has a connection dialog; Ferris IDEA edits connections in
`connections.toml`. Unlike the reference, all non-system schemas are listed without ticking them in the
data source settings (IMP-0052), and a failed introspection shows its cause in the tree with a retry
instead of an empty node (IMP-0051). Its Database icon is not open; the stripe
uses the open `dataSchema` icon. v5: the DDL tab is named "<name> (DDL).sql" (a file in Ferris IDEA's cache, so
the editor highlights SQL; on SQLite the text is the stored statement in the user's own spelling, v7) and Ferris IDEA gives Show DDL a shortcut of its own, ⌥⇧⌘D, since the
reference's key for it opens the data here. **v6:** the quick documentation appears at once from what
the tree read, where the reference waited minutes for an unrelated engine (IMP-0367); it is a panel
above the tree instead of a floating popup; the toolbar has no comparison and view-options buttons
(comparison stays in the context menu, SPEC-0110).
