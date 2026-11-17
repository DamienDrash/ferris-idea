# SPEC-0110: Query plan and schema comparison in the window

- **Version:** 15 · **Date:** 2026-11-16 (v15: the diagram opens fitted but not below 60 %, starting at its top step; Fit still fits it wholly, behaviour 22, AK-27 — OBS-0070 P3); 2026-11-16 (v14: the head of the Plan tab stays while the view scrolls, several hints take one line, the diagram is fitted into its area, behaviour 21, AK-26 — IMP-0450, window check of the plan views, OBS-0070 V6 P2); 2026-11-16 (v13: the Plan tab's Diagram and Flame views and the properties of a step, behaviours 19 and 20, AK-23 to AK-25 — F-059, OBS-0070 V6); 2026-11-16 (v12: Compare With… compares two single tables or views, behaviour 18 — OBS-0068 L8); 2026-11-16 (v11: every script names its target schema, on PostgreSQL only
  for its own transaction, behaviour 16 — IMP-0415, SPEC-0018 v7 behaviour 19; v10: statements tied to
  a table drop in the list, constraints in the differences, both schema names given to the migration,
  behaviours 4, 7 and 16 — OBS-0068 L5–L7; v9: a hint names its step once, behaviour 2 — OBS-0068 L11);
  2026-11-15 (v8: the script file runs on the target, the data-loss question scrolls into view, behaviours 5 and 7 — hand tests 37 and 121 retest; v7: two schemas of one connection, source and target each a connection and a schema, dialect names as written, behaviours 16–17 — OBS-0051 L4; v6: a selection that is no whole statement explains the statement at the caret, behaviour 15 — OBS-0051 L3); 2026-11-14 (v1 2026-11-01; v2: choosing statements, destructive ones
  confirmed by name — IMP-0055; v3: the target is always chosen, the confirmation wraps and answers to
  the keyboard — hand tests 37 and 121, D13, D44; v4: the confirmation takes the keyboard and is
  answered only while it is on screen — hand test 121, D44; v5: entry points in the editor's and the
  Database window's menus, the raw plan, reading and copying a step, the statement list by keyboard,
  swapping the sides, cancelling)
- **Sources:** GEN (database IDEs show the plan of the statement at the caret as a tree with costs and
  rows, optionally measured, and compare two data sources, list the differences and offer the
  migration script to run; F-058, F-059), SPEC-0018 (comparison and DDL), SPEC-0020 (plans),
  SPEC-0093 (SQL console and Services window), SPEC-0094 (machine state under `.idecraft/`), ADR-0008,
  OBS (OBS-0010 A2: the reference shows source and target and can exchange them; OBS-0026 L4, L11 and
  L32: plan values cut off, the plan command not yet compared, the comparison missing from the
  Database window; v6: OBS-0051 L3, a selected word was explained and refused with a wrong reason; v7:
  OBS-0051 L4, two schemas of one connection could not be compared and the message wrote the dialect
  as "PostgreSql"; v10: OBS-0068 L5–L7, constraints lost, foreign keys of kept tables dropped, a view body
  kept the source schema)
- **Feature matrix rows:** F-058, F-059

## Purpose
Read a query plan and compare two schemas from the window, without an agent or the MCP tools, and get a
migration script that the SQL console can run.

## Behaviour
1. **Explain:** `Sql.ExplainPlan` and `Sql.ExplainAnalyze` (Search Everywhere) take the statement at the
   caret of a `.sql` editor, or the selection, as the console does (SPEC-0093 behaviour 1); exactly one
   statement, otherwise the status bar says so. The connection is the console's (SPEC-0093 behaviour
   2). The plan is read in the background and shown in the Services window. [GEN, SPEC-0020]
2. **Plan tab:** a tab "Plan" after the result tabs shows the hints first (each on its own line), then
   one row per plan step in tree order with its depth as indentation: operation, object, details, then
   estimated rows and cost, and for a measured plan actual rows, time in ms and loops. Full scans are in
   the warning colour. A measured plan says whether the statement's changes were rolled back. The output
   log gets one line: "Plan of <first line of the statement>" with the number of steps, or the error.
   [GEN, SPEC-0020] **v3:** the object column is as wide as its longest
   text (180–480 px, longer ends in `…`), so generated index names stay readable (hand test 43).
   **v9 (OBS-0068 L11):** a hint names its step once: its operation and object (`Seq Scan item i`),
   or the operation alone for a step without an object (`Sort`, not `Sort Sort`).
3. **Compare:** `Sql.CompareSchemas` compares the console's connection (source) with another connection
   of the same dialect (target). A popup lists the candidates (↑/↓, Enter, Escape), each with where it
   points (host, port and database, or the file); with none the status bar says so. **v3:** the popup
   appears for a single candidate too, so a connection is never compared (and its password never
   asked for) without the user choosing it. Both schemas are read in the background. [GEN, SPEC-0018]
4. **Diff tab:** a tab "Schema Diff" lists "+ table" (only in the source), "− table" (only in the target)
   and "~ table" with indented lines for added (+), removed (−) and changed (~ old → new) columns,
   primary key, indexes and foreign keys; equal schemas say "No differences". Below it the migration
   script that turns the target into the source, one statement per line block, each ending in `;`,
   statements that can lose data preceded by a comment saying so. [GEN, SPEC-0018]
   **v10:** a changed table also lists its check and unique constraints added (+) and removed (−),
   `constraint <name> <definition>` (SPEC-0018 v6 behaviour 16). [GEN]
5. **Script file:** the script is also written to `.idecraft/migrations/<source>-to-<target>.sql`
   (names made file-safe) and the status bar names it; `Sql.OpenMigrationScript` opens it in the editor,
   where the console runs it on the target after choosing that connection. **v8:** the script file
   is bound to the target connection as its data source (SPEC-0192), so running it in the editor
   goes to the target, whatever the Database window has selected (before, the selected connection —
   often the source — ran it). Nothing is run by the
   comparison itself. [GEN, SPEC-0094]
6. **MCP:** agents already have `explain_sql` and `compare_schemas` (with `include_destructive`, SPEC-0018
   v2); no new tool. [ADR-0008]
7. **Choosing statements (v2):** below the differences the tab lists every statement of the migration
   (SPEC-0018 v2 behaviours 11–14) with a check box; destructive ones are marked "data loss" in the
   error colour and start unchecked. Clicking a statement toggles it. Checking a destructive statement
   first shows a confirmation line naming the objects whose data is lost (for example "drop table
   item: all its rows are lost") with Include and Cancel; only Include checks it. The script below and
   the script file follow the choice at once. `Sql.IncludeDestructiveChanges` checks all destructive
   statements after the same kind of confirmation listing all objects. [GEN, OBS: IMP-0055]
   **v3:** the confirmation text wraps over as many lines as it needs, Include and Cancel sit on their
   own line below it, and Enter includes while Escape cancels. [GEN]
   **v10:** the statements that only exist for a table drop (its foreign key drops, a kept table's
   foreign key to it, SPEC-0018 v6 behaviour 17) start unchecked like the drop; a click on any of
   them asks the question for the whole group, which names the tables and views whose data or
   definition is lost, and leaving one out leaves the group out. [GEN]
   **v8:** when the question appears the Schema Diff tab scrolls it to the top of its area, so it is
   on screen with Include and Cancel however many differences and statements come before it
   (before, it could sit below the visible rows). [GEN]
   **v4:** `Sql.IncludeDestructiveChanges` brings the Schema Diff tab to the front and gives the
   Services window the keyboard, so Enter and Escape answer the question from wherever the action was
   called (in v3 they went to the editor, where Enter typed a line break). Enter and Escape answer
   only while the Schema Diff tab is shown: a question that cannot be read is never confirmed.
   [GEN]
8. **Views in the comparison (v2):** both schemas are read with their view definitions, so the diff
   lists "+ view", "− view" and "~ view". [SPEC-0018 v2]
9. **Entry points (v5):** Explain Plan and Explain Analyze are also in the context menu of the text
   of a `.sql` editor (not in other files). Compare Schemas… is also in the context menu of the
   Database tool window, on a connection and on everything below it; the source is then the
   connection of the selected row. The actions stay in Tools › Database and in Find Action. [GEN,
   OBS-0026 L32]
10. **Raw plan (v5):** the Plan tab starts with a line holding two switches, Tree and Raw. Raw shows
    the plan as the database reported it (SPEC-0020 behaviour 2), line by line and unshortened; Tree is
    the table of behaviour 2 and the default for every new plan. While the Services window has the
    keyboard ⌥R (Alt+R) changes between the two. [GEN, STD]
    **v13:** four switches, Tree, Diagram, Flame and Raw, and ⌥R goes to the next (behaviour 19).
    Before v13 there were two.
11. **Reading and copying a step (v5):** in Tree a click or ↑/↓ selects a step (↓ first selects the
    first step); the selected row has the selection colour. Below the table the selected step is
    written out in full and wrapped — operation and object, every detail on its own line, the
    estimate and the measured values — so what the table cuts off can be read without resizing
    anything. ⌘C (Ctrl+C) copies the plan: in Raw the raw text, in Tree one line per step, indented by
    two spaces per level, with object, estimate, measured values and details. [GEN, OBS-0026 L4]
    **v13:** the step written out in full below the table becomes the list of the step's
    properties (behaviour 20), also below Diagram and Flame; ↑/↓ also move in those views.
12. **Cancelling (v5):** while a plan is being read the Cancel button of the SQL toolbar (SPEC-0093 v3
    behaviour 12) ends the statement in the database as it does for a console statement; this matters
    for Explain Analyze, which runs the statement. Output then shows the database's error.
    `Sql.CancelComparison` (Tools › Database, Find Action; the same Cancel button while a comparison
    runs) gives a running comparison up: the Services window stops waiting, Output says that the
    comparison was cancelled, no Schema Diff tab and no script file come from it, and what the
    read-only schema reads still return is thrown away. Without a comparison in progress the status
    bar says so. [GEN]
13. **Sides and swapping (v5):** the Schema Diff tab starts with a line that names the direction —
    the target becomes like the source, both by connection name — with the buttons Swap and Open
    Script. Swap (`Sql.SwapComparison`, also in Tools › Database and Find Action) compares again with
    source and target exchanged: both schemas are read again, the statement choice starts over with
    every destructive statement unchecked, and the script goes to the file of the new direction
    (behaviour 5). Open Script is `Sql.OpenMigrationScript`. Without a comparison Swap says so in the
    status bar. [GEN, OBS-0010 A2]
14. **Statement list by keyboard (v5):** while the Schema Diff tab is shown and the Services window
    has the keyboard, ↑/↓ move a marker over the statements (↓ first marks the first one) and Space
    toggles the marked statement exactly as a click does, so checking a destructive statement asks
    first and Enter or Escape answer (v3, v4). The marked row has the selection colour. While the
    question is open ↑/↓ and Space do nothing. [GEN]
15. **What is explained (v6, OBS-0051 L3):** a selection is explained only when it is made of whole
    statements of the file (each selected statement starts where one of the file's statements starts
    and has its text; blanks and comments around it do not count). A selection that is not — a word
    such as `FROM`, half a statement — is set aside: the statement at the caret is explained
    (SPEC-0093 v6 behaviour 1), and the status line says that the statement at the caret was taken
    because the selection is not a whole statement. With no statement at the caret nothing runs and
    the status line says exactly that: the selection is not a whole statement and the caret is on no
    statement. A refusal names its real reason; the refusal for schema changes (behaviour 2,
    SPEC-0020) is given only for a statement that is one. Running a selection (Execute) is unchanged.
    [GEN]
16. **Two schemas, also of one connection (v7, OBS-0051 L4):** on PostgreSQL, DuckDB, MySQL and MariaDB
    source and target are each a connection and a schema. Compare Schemas asks in up to three popups
    of behaviour 3, each only when there is a choice: the source's schema (when its connection has
    more than one; the current one first), the target connection (the other connections of the same
    dialect and, listed first as "<connection> · other schema", the source's own connection), and the
    target's schema (when that connection has more than one; the source's schema is left out when the
    target is the source's connection). A connection whose schemas cannot be listed offers its current
    schema alone and the status line says why. Both schemas are read with their tables and view
    definitions (behaviour 8). A side is named `<connection>` for its connection's current schema and
    `<connection>.<schema>` otherwise, in the Schema Diff tab, the Output line, the script's head and
    the script file's name. A script whose target is not the target connection's current schema starts
    with the statement that makes it current (`SET search_path TO …` on PostgreSQL and DuckDB,
    `` USE `…` `` on MySQL and MariaDB), so it runs on that schema from any console of the connection.
    Swap (behaviour 13) exchanges both sides with their schemas. On SQLite, SQL Server and Oracle the
    comparison stays between connections. `compare_schemas` takes `source_schema` and `target_schema`
    (default: each connection's current schema) and refuses the same connection and schema on both
    sides. [GEN]
    **v10:** both read the name of a side's current schema from its connection and give the
    migration both schema names, so view bodies are written for the target and compared as SPEC-0018
    v6 behaviour 18 says, also between the current schemas of two connections. [GEN]
    **v11:** every script names its target schema, also when it is the target connection's
    current one, as SPEC-0018 v7 behaviour 19 says: on PostgreSQL the first statement inside the
    script's transaction is `SET LOCAL search_path TO <target>, pg_temp`, so running the script
    leaves the console's `search_path` as it was; DuckDB, MySQL and MariaDB start with the
    statement of v7, after a comment line saying that it stays for the session. The window's head
    line comes before it; a line break in a name it shows is written as a space, so a schema or
    connection name cannot end the comment and become a statement. Before v11 a script for the
    current schema had no such statement, and the one for another schema set it for the whole
    session: running the reverse script first left the console in its target, and the next
    script, made for the connection's current schema, then created and changed objects in that
    schema (IMP-0415). [GEN, STD]
17. **Dialect names as written (v7, OBS-0051 L4):** messages name a dialect as it is written
    (PostgreSQL, MySQL, MariaDB, SQLite, SQL Server, Oracle, DuckDB), never as the program's internal
    name. With no second connection and a single schema, the status line says that there is nothing
    to compare `<connection>` with: no other <dialect> connection and no other schema. [GEN]
18. **Two single objects (v12, OBS-0068 L8):** `Sql.CompareObjects` ("Compare With…" in the context
    menu of a table or view in the Database window, also in Tools › Database and Find Action)
    compares the object selected in the Database window (the source, in its connection's current
    schema) with one other object, as SPEC-0018 v8 behaviour 20 says. Popups of behaviour 3 ask
    in turn: the target connection (the source's own connection first, then the other connections
    of the same dialect), the target's schema (behaviour 16, when that connection has more than
    one; here the source's schema is offered too), and the object among that schema's tables or
    views of the same kind — an object of the same name first, the source object itself left out.
    Without a selected table or view the status bar says to select one. The result is the Schema
    Diff tab of behaviours 4, 7 and 13–14 for the two objects: the head line names each side as
    `<side>.<object>` (with the side of behaviour 16), destructive statements start unchecked and
    are included only after the confirmation, the script is transactional where the dialect allows
    it and pins the target schema (v11), and it is written to the file of behaviour 5 named after
    both sides and objects and bound to the target connection (v8). Swap exchanges the two objects
    with their sides. `compare_schemas` takes `source_object` and `target_object` (both or
    neither) for the same comparison; then the same connection and schema on both sides is allowed
    when the objects differ. [GEN, ADR-0008]

19. **Four views of a plan (v13, F-059, OBS-0070 V6):** the switches of behaviour 10 are Tree,
    Diagram, Flame and Raw; ⌥R (Alt+R) goes to the next of them, from Raw back to Tree. Tree stays
    the default for every new plan.
    - **Diagram:** each step is a box with its operation, object and its share of the plan's cost
      (SPEC-0020 v2 behaviour 8, e.g. `62 %`); the top-level steps stand at the top, each step's
      children in a row below it, and a line leads from every child to its parent. A box is as
      wide as its text; the diagram scrolls when it is larger than the tab.
    - **Flame:** each step is a bar across the share of its parent that its value takes, the
      top-level steps over the whole width and the children in the row below their parent. The
      value is chosen with three switches: Total cost (the default), Startup cost and, for a
      measured plan, Time (measured time × loops); a step without that value takes the sum of its
      children's, and a plan without any (SQLite) gives each step an equal part of its parent. A
      bar names its operation and object as far as it fits, its value and share follow in the
      properties.
    - **Steps and hints in every view:** a click on a row, box or bar selects that step in all
      views; ↑/↓ move through the steps in tree order in Tree, Diagram and Flame. A step with a
      hint (full scan, misestimate, SPEC-0020 behaviour 5) has the warning colour and a ⚠ in every
      view, and its hint is also written at the step in the properties. The list of all hints
      stays at the top of the tab. **v14:** several hints fold to one line (behaviour 21).
    - **v14:** the diagram is fitted into its area, with 100 % as the other choice (behaviour 21);
      it scrolls only when it is larger than its area at that zoom.
    Before v13 the tab had Tree and Raw only. [GEN, OBS-0070 V6]
20. **Properties of the selected step (v13):** below Tree, Diagram and Flame the selected step's
    properties are listed, one per line as `name: value`, with every field the database reported
    for it (SPEC-0020 v2 behaviour 7), named as the database names them, then its share of the
    plan's cost and its hints. This replaces the written-out step of behaviour 11 below the table;
    ⌘C (Ctrl+C) still copies the whole plan (in Diagram and Flame as in Tree). `explain_sql`
    returns the same: each step with `properties`, `startup_cost`, `cost_share` and `hints`
    (SPEC-0020 v2, ADR-0008). Explain Analyze runs every statement inside a transaction that is
    rolled back (SPEC-0020 v2 behaviour 4), and the tab says so. [GEN, ADR-0008]
21. **Head of the Plan tab, hints and fitting (v14, IMP-0450, OBS-0070 V6 P2):** the Plan tab is a
    head and a view below it. The head holds the switches (behaviours 10 and 19, with the Flame
    metrics or the diagram's zoom switches), the key hints, the note that a measured plan was
    rolled back (behaviour 20) and the hints; it stays at the top while the view below it scrolls:
    the rows of Tree, the boxes of Diagram or the bars of Flame with the selected step's
    properties, or the raw text. Before v14 the switches and hints scrolled away with the view.
    - **Hints:** one hint is written in full, as before. Several hints take one line: the first,
      cut with `…` at the edge of the tab, and at the end of the line how many more there are
      (`2 more hints ▾`). A click on that line, or ⌥H (Alt+H) while the Services window has the
      keyboard, shows every hint in full, each on its own line, and the line then ends in
      `fewer hints ▴`; the same click or key folds them again. Every new plan starts folded. A
      step's hint stays in its properties and its ⚠ in every view (behaviours 19 and 20). Raw
      shows no hints, as before. The key hints name ⌥H while there is something to fold.
    - **Fitting the diagram:** the Diagram view has two switches, Fit (the default for every new
      plan) and 100 %. Fit draws the diagram at the zoom the ER diagram fits with when it opens
      (SPEC-0111 behaviour 8): the largest zoom, at most 100 % and at least 25 %, at which the
      whole diagram is inside the area below the head. Boxes, text, gaps and lines scale
      together. A plan so deep that it is larger than the area even at 25 % is drawn at 25 % and
      the view scrolls the rest, as the ER diagram does. While Fit is chosen the diagram is
      fitted again whenever that area changes size (the tool window is resized, the hints are
      opened or folded), in the frame after the one that measured it (SPEC-0111 v7). 100 % draws it at its full size, and it scrolls down and
      sideways as in v13; a wheel turn down scrolls the view and never the diagram sideways, a
      sideways swipe scrolls the diagram and never the view.
    - **Not built (v14):** the plan's diagram has no zoom steps, no ⌘= / ⌘- / ⌘0 / ⇧⌘0 and no
      zoom with ⌘ and the wheel, which the ER diagram has (SPEC-0111 behaviour 7); Fit and 100 %
      are its only zooms. Nor is there a layout from left to right, which would use a wide, low
      tool window better than rows from top to bottom (a plan five steps deep stays at 25 % in a
      tool window of normal height). Flame needs no zoom: its bars already span the width of the
      tab.
    This changes only how the tab is laid out; agents read the same plan, hints included, with
    `explain_sql` (behaviour 20), so there is no new MCP tool. The reference was not observed for
    this (whether it keeps its switches in place is open). [GEN, ADR-0008]
22. **Readable when it opens (v15, OBS-0070 P3):** in the window check of 2026-11-16 a plan of six
    steps in a tool window of normal height (about 220 px below the head) was fitted so small that
    no box could be read; at 100 % everything could. The Diagram view now opens — for every new
    plan, and when Diagram is chosen for it — at the zoom of Fit (behaviour 21), but not below
    60 %, the size at which the boxes' text stays readable. A diagram that is larger than its area
    at that zoom starts at its top step: the view is at its top, and a diagram wider than the area
    is scrolled sideways so that the top step stands in the middle (never before the diagram's
    start or past its end); the rest scrolls as at 100 %. While it is shown that way it is fitted
    again, with the same lower limit, whenever the area changes size. A click on Fit still fits
    the whole diagram into the area, down to 25 %, as behaviour 21 says; 100 % is unchanged. The
    Fit switch is shown chosen in both fitted states. [GEN; deviation from v14]

## Acceptance criteria
- **AK-1** Plan rows: depth, columns, full-scan mark and measured values from a nested plan; script text
  with data-loss comments and `;`; diff lines for added, removed and changed tables, columns, keys,
  indexes and foreign keys; equal schemas; candidate targets of the same dialect; file-safe script
  names (unit tests `db_compare_model::tests`).
- **AK-2** Window self-test on SQLite: Explain on a `select` with a join shows the Plan tab with steps;
  Compare Schemas between two SQLite connections that differ by a table and a column shows the
  differences, writes the script, and the script run on the target leaves no differences.
- **AK-3** Hand test (round 4): Explain and Compare on PostgreSQL next to the reference.
- **AK-4 (v2)** Statement list, default selection, confirmation text and script per choice (unit tests
  `db_compare_model::tests`).
- **AK-5 (v2)** Window self-test on SQLite: comparing a target that has an extra table with a view on
  it lists `DROP VIEW` and `DROP TABLE` unchecked; including them asks for confirmation; the script
  then runs on the target without error.
- **AK-6 (v2)** Hand test 121: compare two PostgreSQL schemas both ways next to the reference.
- **AK-7 (v3)** Window self-test: with a single candidate Compare Schemas shows the popup (`copy · …`)
  and compares only after Enter; the data-loss confirmation is answered with Enter; unit test of the
  target labels (`db_compare_model::tests`).
- **AK-8 (v4)** Window test without a screen on SQLite, and the window self-test:
  `Sql.IncludeDestructiveChanges` called with the editor focused and the Output tab in front shows
  the Schema Diff tab and the Services window has the keyboard; Escape cancels the question and
  nothing is included; Enter on another tab leaves the question open; called again, Enter includes
  every statement and the editor's text is unchanged
  (`schema_diff_confirmation::the_data_loss_question_takes_the_keyboard`).

- **AK-9 (v5)** The editor's menu has Explain Plan and Explain Analyze for a `.sql` file and not for
  another file; the Database window's menu has Compare Schemas… enabled on a connection, a folder, a
  table and a column and disabled on nothing
  (`context_menu::tests::database_entry_points_for_plan_and_comparison`).
- **AK-10 (v5)** The copied tree text of a nested, measured plan (indentation, object, estimate,
  measured values, details) and the lines of a selected step in full
  (`db_compare_model::tests::plan_text_and_step_lines`); moving the statement marker with its ends
  (`db_compare_model::tests::the_statement_marker_moves_within_the_list`).
- **AK-11 (v5)** Window test without a screen on SQLite
  (`plan_and_compare_keys::the_plan_is_read_by_keyboard`): Explain Plan on a join shows the Plan tab
  in Tree; ↓ selects the first step; ⌥R shows the raw text, which holds the database's own lines;
  ⌘C puts it on the clipboard; ⌥R goes back.
- **AK-12 (v5)** Window test without a screen on SQLite
  (`plan_and_compare_keys::the_comparison_swaps_and_answers_to_the_keyboard`): Compare Schemas from
  the Database window's menu action; ↓ and Space on a destructive statement ask, Escape leaves it
  unchecked, Space and Enter check it and the script has the statement; Swap exchanges source and
  target, the differences change their sign, every destructive statement is unchecked again and the
  script file is the one of the new direction.
- **AK-13 (v5)** A cancelled comparison leaves no Schema Diff tab and Output says so; a result that
  arrives after the cancel changes nothing
  (`plan_and_compare_keys::a_cancelled_comparison_shows_nothing`).
- **AK-14 (v5)** Hand tests 37 and 121 again, with behaviours 9–14, on PostgreSQL and MySQL next to
  the reference.
- **AK-15 (v6, behaviour 15)** `sql_console_model::tests::explain_takes_the_statement_at_the_caret_for_a_partial_selection`:
  a whole selected statement, two whole statements, a selected word with the caret in a statement
  (that statement), a selected word on a line without a statement (none, with the reason), and the
  status line text of both.
- **AK-16 (v7, behaviour 16)** `db_compare_model::tests::{sides_are_named_with_their_schema,
  the_script_starts_in_the_target_schema, targets_offer_the_own_connection_first}`; with two schemas of
  one DuckDB file (in the test process) `workbench_view::tests::two_schemas_of_one_connection_compare`:
  the table only in the source schema is "+ table", the script starts with `SET search_path TO`;
  `crates/mcp/tests/it/protocol.rs::compare_schemas_takes_two_schemas_of_one_connection`.
- **AK-17 (v7, behaviour 17)** `db_compare_model::tests::nothing_to_compare_names_the_dialect` and the
  MCP refusal of two dialects in `compare_schemas_takes_two_schemas_of_one_connection` names "SQLite"
  and "PostgreSQL".
- **AK-18 (v8, behaviours 5 and 7)** In a window of the test harness on SQLite
  (`crates/workbench/tests/it/db_handtest_retest.rs`): `hand_test_37_plan_then_compare_asks_for_the_target`
  — the plan with its full-scan hint, Compare Schemas asks for the only target, Open Migration
  Script opens the script bound to the target; `hand_test_121_views_and_the_data_loss_question` — the
  question wraps, it and Include and Cancel are drawn inside the window, Enter includes, the script
  runs on the target and a new comparison finds no differences.
- **AK-19 (v9, behaviour 2)** A step without an object is named once in its hint
  (`plan::tests::a_hint_names_a_step_once`).
- **AK-20 (v10, behaviours 4, 7, 16)** `db_compare_model::tests::diff_lines_list_tables_and_their_parts`
  has the constraint lines; `db_compare_model::tests::statements_tied_to_a_drop_ask_with_it`: a click on
  the foreign key drop of a dropped table asks for the group, the question names the table and not the
  foreign key, Include checks the group, a click leaves it out again; the window and
  `compare_schemas` pass both schema names (`workbench_view.rs` `run_compare`, MCP `compare_schemas`;
  `crates/mcp/tests/it/protocol.rs::compare_schemas_takes_two_schemas_of_one_connection`: a DuckDB view
  on `<source schema>.t` is created on `<target schema>.t`).
- **AK-21 (v11, behaviour 16)** `db_compare_model::tests::the_script_starts_in_the_target_schema`:
  the PostgreSQL script for another schema and for the current one pins its target with
  `SET LOCAL search_path` right after `BEGIN`, the MySQL script with `USE` after the head line and
  its comment, and a target schema name with a line break stays inside the head line;
  `workbench_view::tests::two_schemas_of_one_connection_compare` (DuckDB) and
  `crates/mcp/tests/it/protocol.rs::compare_schemas_takes_two_schemas_of_one_connection`: the first
  statement is `SET search_path TO "old"`; the run on PostgreSQL is SPEC-0018 v7 AK-17.
- **AK-22 (v12, behaviour 18)** The popups' order and the object list
  (`db_compare_model::tests::object_targets_and_labels`); in a window of the test harness on SQLite
  (`crates/workbench/tests/it/db_compare_objects.rs`:
  `compare_with_compares_two_tables`): Compare With… on a table, the target connection and the
  table chosen by Enter, the Schema Diff tab names both objects, the drop of a column starts
  unchecked and the script changes only the target table; Swap exchanges the objects. MCP
  (`crates/mcp/tests/it/protocol.rs`: `compare_schemas_compares_two_objects`): two tables of one
  SQLite file under different names, the refusals for a missing object, for only one of
  `source_object`/`target_object` and for a table against a view.
- **AK-23 (v13, behaviour 19)** Unit tests `plan_view_model::tests`
  (`diagram_places_children_below_their_parent`, `flame_widths_follow_the_metric`,
  `views_cycle_and_steps_move_in_every_view`): boxes, edges and bars of a nested PostgreSQL plan;
  bars by total cost, start-up cost and measured time, equal parts without costs; ⌥R's order.
- **AK-24 (v13, behaviour 20)** `plan_view_model::tests::properties_of_a_step`: every field, the
  share and the hint of a full scan, in order.
- **AK-25 (v13, behaviours 19 and 20)** In a window of the test harness on SQLite
  (`crates/workbench/tests/it/plan_views.rs`: `diagram_flame_and_properties`): Explain on a join,
  Diagram and Flame by click and by ⌥R, a click on a box selects the step in every view and the
  properties list it. Written, runs on the Mac.
- **AK-26 (v14, behaviour 21)** Unit test
  `plan_view_model::tests::the_diagram_fits_its_area_and_hints_collapse`: the diagram's size at
  100 %, the fitted zoom by width and by height, never above 100 % or below 25 %, 100 % when
  chosen; the first hint with the count, all hints when open, one hint unfolded. In a window of the
  test harness on SQLite (`crates/workbench/tests/it/plan_views.rs`:
  `the_head_stays_and_the_diagram_fits`), a plan five rows deep with three full scans: the hints
  take one line ending in `2 more hints ▾`, ⌥H shows each on its own line, a click folds them; in
  Diagram the zoom is the fit of the measured area and below 100 %, and every box is whole in
  view; at 100 % a wheel turn moves the boxes up but not sideways, and the switches and the hint
  line stay where they were; Fit again and a taller window fit it larger with every box whole,
  a low window down to 25 %; no key reaches the editor. Runs on the Mac.
- **AK-27 (v15, behaviour 22)** Unit test
  `plan_view_model::tests::the_diagram_fits_its_area_and_hints_collapse`: as it opens the zoom is
  the fit where that is 60 % or more and 60 % below it, 100 % while the area is not known; six
  steps in a column in an area 220 px high fit below 40 % with Fit and open at 60 %; a diagram
  wider than its area starts with the top step in the middle, never before its start or past its
  end. In a window of the test harness (`plan_views.rs`: `the_head_stays_and_the_diagram_fits`,
  macOS; changed, not yet built): as it opens the zoom is the fit but at least 60 % and the top
  step is whole in view; a click on Fit fits the whole diagram with every box whole.

## Out of scope
Objects SPEC-0018 leaves out, comparing a database
with DDL files, running the script from the diff tab, a side-by-side DDL diff per object, choosing the source
connection in the popup. v7: two schemas of one connection on SQLite (attached databases), SQL Server
and Oracle.

## Known deviations from the reference
Destructive statements start unchecked and need a confirmation naming the objects; the script is
ordered by dependency and transactional where possible (IMP-0055). The comparison writes a script
file instead of offering a separate "synchronize" dialog, so the change is reviewed and run like any
other SQL.
- Swapping reads both schemas again instead of reversing what is on screen, so the script never
  comes from a stale read. The reference's per-object property and DDL diff panes and its Execute
  button are not built: the script is reviewed in the editor and run from there (v5).
- Whether the reference offers the measured plan and the raw plan from the same places, and how its
  plan view is laid out, is still to be compared (OBS-0026 L11).
- v13 (IMP-0430): steps keep the names the database gives them (`Seq Scan`, `Table scan`, `SCAN`) in every view
  and in the properties, instead of one vocabulary for all databases, so a step can be looked up in
  the database's own documentation. The hints (full scan, misestimate) stand at their step in every
  view, not only in a list. Every measured statement is rolled back, also a read (SPEC-0020 v2).
- v14 (IMP-0450): the head of the Plan tab — switches, key hints, the rolled-back note and the
  hints — stays at the top while only the view below it scrolls; several hints fold to one line
  with a count and open by a click or ⌥H; the diagram is fitted into the area below the head when
  it is shown and whenever that area changes, as the ER diagram fits when it opens, with 100 % as
  the other choice. The reference was not observed for this (OBS-0070 V6 P2), so this is our own
  choice and not taken from it.
