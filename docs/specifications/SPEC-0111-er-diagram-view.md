# SPEC-0111: ER diagram in the window

- **Version:** 8 · **Date:** 2026-11-16 (v8: lines are led around the boxes that are not their ends, keeping 8 px from
  them, behaviour 16 — also a route of behaviour 3 that runs closer, window check of #867 E1 — and the export menu
  offers yEd GraphML and draw.io, behaviour 11 — OBS-0068 L9, L10); 2026-11-15 (v7: the diagram is fitted in the frame after the first one, behaviour 8 — hand test 120 retest; v6: lines share no vertical segment, behaviour 15 — OBS-0051
  L5); 2026-11-14 (v1 2026-11-01; v2: moving boxes, zoom, fit, layout, views,
  export — IMP-0053, IMP-0054; v3: boxes wide enough for name and type, fit follows the area until
  zoomed by hand — hand test 120, D43; v4: ⇧⌘0 fits on every keyboard layout — hand test 120; v5:
  foreign-key lines name their columns, boxes and the export menu by keyboard, reading the schema
  can be given up)
- **Sources:** GEN (database IDEs draw the tables of a connection or a selection as boxes with their
  columns and key marks and the foreign keys as lines between them, opened from the database tree with
  ⌥⇧⌘U on macOS and Ctrl+Alt+Shift+U elsewhere; F-056), SPEC-0022 (ER model and layered layout),
  SPEC-0089 (Database tool window), OBS (OBS-0010, A1; OBS-0068 L9, L10), ADR-0008
- **Feature matrix rows:** F-056

## Purpose
See how the tables of a database hang together without leaving the window or exporting Mermaid.

## Behaviour
1. **Opening:** `Sql.ShowErDiagram` (Search Everywhere) or ⌥⇧⌘U (Ctrl+Alt+Shift+U) in the Database
   window. On a connection (or one of its groups) all its tables are drawn; on a table that table and
   its neighbours over one foreign key (SPEC-0022 behaviour 1, depth 1). The schema is read in the
   background; the diagram shows over the editor area. Without a connection the status bar says so.
   [GEN, SPEC-0022]
2. **Boxes:** one box per table at its layer and row (SPEC-0022 behaviour 4): referenced tables left,
   layers as columns, rows top to bottom with a fixed gap. The header holds the table name; each column
   is a line with its key marks (`PK`, `FK`, `UK`), name and type, nullable types dimmed. Box width
   follows the longest line within limits. **v3:** the width is computed from the parts as they are
   drawn — inset, key column (as wide as the longest key marks, at least `PK FK`), name, a gap, the
   type right-aligned, inset — so name and type never overlap; up to the widest box (480 px) nothing
   is cut, and beyond it the name is shortened with `…` while the type stays whole. Window, SVG and
   PNG use the same geometry. [GEN]
3. **Lines:** each relationship is drawn from the right edge of the referenced table's box to the left
   edge of the referencing table's box, at the rows of the first referenced and referencing column,
   with an elbow halfway between the layers; a self-reference loops out to the right of its box. The ends
   are labelled with the cardinality: `1` or `0..1` at the referenced end, `N` or `1` at the referencing
   end. **v8:** this is the route of a line whose way is clear; where it would cross another box the
   line goes around it (behaviour 16). [GEN, SPEC-0022 behaviour 3]
4. **Interaction:** the diagram scrolls in both directions. Clicking a box selects it; the selected box
   and its lines use the selection colour. The header names the connection and counts tables and
   relationships, and lists foreign keys to tables outside the diagram. Escape or × closes it. [GEN]
5. **MCP:** `er_diagram` gives agents the model and every export format of SPEC-0022 v2. Format
   `layout` returns the drawing of this view (boxes, lines, canvas size) and, for a given viewport, the
   fit zoom; `positions` (table → x, y) moves boxes before `layout`, `svg` and `png` are produced, as
   dragging does in the window. **v8:** also `graphml` and `drawio` (SPEC-0022 v4), which take
   `positions` too; the lines of `layout` are the routes of behaviour 16. [ADR-0008]
6. **Views (v2):** views are boxes too, marked `view` in the header and drawn with a dashed border; a
   dashed line goes from each table or view a view uses to the view. They take part in the layering
   like tables (a view is right of what it uses). [GEN, SPEC-0022 v2]
7. **Zoom (v2):** a toolbar in the header has zoom out, the zoom in percent, zoom in, actual size (1:1)
   and fit. Zoom goes in steps of ×1.25 between 25 % and 400 %; ⌘= / ⌘+ zooms in, ⌘- out, ⌘0 to 100 %,
   ⇧⌘0 fits; ⌘ with the scroll wheel zooms too. Boxes, text, lines and labels scale together. [GEN]
8. **Fit (v2):** fit picks the largest zoom (at most 100 %) at which the whole canvas is visible in the
   diagram area. A diagram is fitted when it opens, so no table is cut off. **v3:** after opening or
   an explicit fit (or layout) the diagram keeps fitting when its area changes size — while the window
   settles, or when a tool window is resized — until the zoom is set by hand (zoom in/out, 1:1, ⌘ +
   wheel). **v7:** the fit happens in the frame right after the one that measured the area; before,
   it waited for an unrelated redraw (the window asked for a frame while painting, which is
   ignored), so the diagram could open at 100 % with boxes cut off. ⇧⌘0 fits whether the system reports it as `⇧0` or as the shifted
   character `)`. **v4:** the zoom keys go by the place of the key where the layout types another
   character there, as other shortcuts do (SPEC-0071 v4 behaviour 6): on layouts where ⇧0 types `=`
   (German) that key fits instead of zooming in, and zooming in is ⌘+, which has a key of its own
   there; on layouts with the digits on the shifted row ⇧⌘0 fits and ⌘ with the unshifted key is
   100 %. v3 zoomed in on ⇧⌘0 there (100 % became 125 %, hand test 120). [GEN, OBS: IMP-0053]
9. **Moving boxes (v2):** dragging a box by any point moves it with the mouse (in canvas units, so it
   follows at every zoom); its lines follow, re-routed from its new edges (a line leaves the side that
   faces the other box). The canvas grows when a box moves beyond it; boxes cannot leave the top or
   left margin. The positions stay while the diagram is open. [GEN]
10. **Layout (v2):** the layout button puts every box back at its layered position (SPEC-0022
    behaviour 4) and fits. [GEN]
11. **Export (v2):** an export menu offers Mermaid, PlantUML, DOT, SVG and PNG (1×, 2×, 4×). Text
    formats are copied to the clipboard and written; every export is written to
    `.idecraft/diagrams/<connection>.<ext>` (name made file-safe), and the status bar names the file.
    Exports are complete (SPEC-0022 v2 behaviour 8) but use the box positions as moved (SVG/PNG). There
    is no web target and nothing is uploaded. **v8:** after PNG 4× the menu offers yEd GraphML
    (`.graphml`) and draw.io (`.drawio`) (SPEC-0022 v4 behaviours 15–17). Both are text formats:
    copied to the clipboard and written to the file, in one step as the others. They carry the boxes
    as moved and the lines with their routes (behaviour 16). The menu order of the earlier entries
    stays, so ↓ ↓ ↓ still marks SVG. A dependency diagram (SPEC-0100 v4 behaviour 15) offers neither,
    as it offers no PlantUML or DOT. [GEN, STD, OBS: OBS-0068 L10, IMP-0054]
12. **Column labels on the lines (v5):** every foreign-key line carries the label of SPEC-0022 v3
    behaviour 14 (the referencing column, with `:` and the referenced column when the two names
    differ) in the small text of the cardinalities, centred on the middle of the line's middle
    segment and just above it. The label scales with the zoom and follows a moved box. View
    dependencies have no label. [GEN, OBS-0010 A1]
13. **Keyboard (v5):** with the diagram focused, Tab selects the next box and ⇧Tab the previous one
    in the order of the layout (layer by layer, top to bottom; from no selection Tab selects the
    first and ⇧Tab the last, and both wrap). ⌥E (Alt+E) opens the export menu with its first entry
    marked; ↑/↓ move the mark, Enter exports the marked format and closes the menu, Escape closes
    only the menu. ⌥L (Alt+L) is Layout. The toolbar buttons name themselves to assistive tools and
    the UI driver (SPEC-0319). [GEN]
14. **Giving up the read (v5):** while the schema is read for a diagram the status bar says so;
    `Sql.CancelComparison` (SPEC-0110 v5 behaviour 12) also gives this read up: no diagram opens
    and the status bar says that it was cancelled. [GEN]
15. **Separate lines (v6, OBS-0051 L5):** no two lines run along the same vertical segment over a common
    height, so each foreign key can be followed to its end. A line whose vertical segment would lie on
    one of a line before it (in the order of the relationships, then the view dependencies) moves aside
    in steps of 8 px, first to the right, then to the left; an elbow between two boxes stays inside
    their gap, a line round the right side of a box only moves outwards. Where the gap has no room
    left the segment keeps its place. **v8:** this holds for every inner vertical segment of a line,
    also of a route around boxes (behaviour 16); a segment moves only to a place where the line
    stays clear of every box as behaviour 16 asks. [GEN]
16. **Lines around boxes (v8, OBS-0068 L9; window check of #867 E1):** the rule is: no segment of
    a line comes closer than 8 px to the rectangle of a box that is not one of the line's two ends
    (it stays outside that rectangle widened by 8 px on every side), and none comes closer than 8 px
    to the line's own boxes except its first and its last segment, which leave and enter them at the
    end points. Where no route keeps 8 px, because boxes stand closer than 16 px to each other or to
    the line's end points, the distance is at least 2 px. Every segment is horizontal or vertical. A
    line keeps the route of behaviours 3 and 9 only while that route keeps the full 8 px; a route
    that runs along a box closer than that (an elbow halfway between a narrow box and the next layer
    beside a wider box of the same layer) is led like one that crosses a box. Such a line leaves and
    enters its boxes at the same points and sides and goes around the boxes in its way: along the
    rows of its end points or on lanes through the middle of the free space between boxes, with the
    fewest bends and the shortest way (a bend counts like 40 px), and of equal routes the one nearest
    the middle between its ends. The inner horizontal segments of such routes keep apart in 8 px
    steps as the vertical ones of behaviour 15 do, so two routes through the same gap do not lie on
    one another. A foreign-key line, a self-reference and a view dependency are led alike. The
    column label (behaviour 12) sits on the middle segment of the route. The rule holds in the
    layered layout and after any box is moved, as long as a way exists: where another box covers an
    end point or leaves it less than 4 px of room (boxes dragged onto each other), the line keeps the
    route of behaviour 3. The window, SVG, PNG, GraphML, draw.io and the MCP format `layout` draw the
    same route. [GEN]

## Acceptance criteria
- **AK-1** Geometry: box positions by layer and row, box sizes from their lines, line end points at the
  right column rows with the elbow between layers, self-reference loop, cardinality labels, total
  canvas size (unit tests `er_view_model::tests`).
- **AK-2** Window self-test on the SQLite fixture: ⌥⇧⌘U on the connection draws `customers` and
  `orders` with one relationship, `customers` left of `orders`; on the table `orders` it draws both
  tables; Escape closes.
- **AK-3** Hand test (round 4): the diagram of the reference database next to the reference.
- **AK-4 (v2)** Zoom steps and limits, fit for small and large canvases, moving a box re-routes its
  lines and grows the canvas, layout resets (unit tests `er_layout::tests`).
- **AK-5 (v2)** Window self-test: the diagram opens fitted (zoom ≤ 100 %, every box inside the visible
  area), ⌘= / ⌘- / ⌘0 change the zoom, dragging `orders` moves it and its line, Layout puts it back,
  the SVG export is written under `.idecraft/diagrams/`.
- **AK-6 (v2)** MCP `er_diagram` with `layout`, `positions` and the fit zoom (protocol test).
- **AK-7 (v2)** Hand test 120: zoom, fit, dragging and export next to the reference.
- **AK-8 (v3)** Geometry for `unit_price_cents integer`, `moved_at timestamp without time zone` and
  `warehouse_id integer` with `PK FK UK`: the name ends at least one gap before the type, no name is
  shortened, and an 80-character name in the widest box ends in `…`
  (`er_layout::tests::boxes_hold_name_and_type_side_by_side`).
- **AK-9 (v4)** The zoom command of a key pressed with ⌘: `)`, `0` with shift, and the key in the
  place of `)` on a German layout (`=`) and on a layout with shifted digits (`0`) fit; `+` and `-`
  zoom on every layout; `=` zooms in only where it is not ⇧0
  (`er_view::tests::shift_cmd_zero_fits_on_every_layout`).

- **AK-10 (v5)** Window test without a screen on SQLite
  (`er_diagram_keys::boxes_and_exports_by_keyboard`): the diagram of a connection with `customers`
  and `orders` has one line labelled `customer_id:id`; Tab selects `customers`, Tab `orders`, Tab
  `customers` again, ⇧Tab `orders`; ⌥E opens the export menu, ↓ ↓ ↓ Enter writes the SVG file, which
  holds the label; Escape with the menu open keeps the diagram, Escape again closes it.
- **AK-11 (v5)** The order Tab follows and the marked export entry with its ends
  (`er_view::tests::tab_follows_the_layout_and_the_export_mark_wraps`).
- **AK-12 (v5)** Hand tests 38 and 120 again, with behaviours 12–14, next to the reference.
- **AK-13 (v6, behaviour 15)** `er_layout::tests::lines_share_no_vertical_segment` (two keys of one
  table to the same table: the first line keeps the middle, the second gets its own segment inside the
  gap; the existing routing tests keep their points).
- **AK-14 (v7, behaviour 8; v3 AK-8 in the window)** In a window of the test harness on SQLite
  (`crates/workbench/tests/it/db_handtest_retest.rs` `hand_test_120_boxes_hold_their_text_and_the_diagram_fits`):
  the diagram opens fitted with the canvas inside its area; name and type, measured in the diagram
  font, keep a gap and no name is shortened; ⌘=, ⌘0 (100 %), ⇧⌘0 fits again; a moved box keeps its
  lines and Layout (⌥L) puts it back; SVG and PNG are written under `.idecraft/diagrams` with whole
  names and types.
- **AK-15 (v8, behaviour 16)** The rule of behaviour 16, measured segment by segment against every
  box rectangle (`er_layout::tests::lines_go_around_boxes`), for a schema in the shape of OBS-0068
  and of the window check of #867: `item`, the wider `pd_note` and the narrow `warehouse` left;
  `movement`, `pd_move`, the view `pd_note_large` and the narrower `stock` in the middle; the view
  `stock_value` at the top right, using `item` and `stock`. Before routing, the route of behaviour 3
  of `warehouse` → `movement` runs 4 px beside `pd_note`, that of `stock` → `stock_value` 2 px
  beside `movement` up to its header row, and that of `stock_value` → `item` over the header of
  `movement`. After routing, every segment is horizontal or vertical and keeps at least 8 px from
  every box that is not its end, and from its own boxes except with its first and last segment;
  routes of behaviour 3 that keep 8 px keep their points up to their lane; no two inner segments of
  one direction lie on one another; the label sits on the middle segment; the SVG has one path per
  line with the same points. After 200 random moves of the boxes (seeded, no two boxes closer than
  16 px) and for a self-reference with a box 16 px beside it, no segment touches a foreign box and
  every one keeps at least 2 px (`er_layout::tests::lines_stay_clear_of_moved_boxes`).
- **AK-16 (v8, behaviours 11, 16)** Window test without a screen on SQLite
  (`er_diagram_keys::lines_go_around_boxes_and_editor_exports`): a schema whose keys and view
  dependency skip a layer draws, in the layered layout, every line at least 8 px from every box that
  is not its end and leads at least one around; after a box is dragged to the far right no line
  touches a foreign box; ⌥E with ↓ to "yEd GraphML" and to "draw.io" and Enter write
  `.idecraft/diagrams/<connection>.graphml` and `.drawio` holding the documents of SPEC-0022 v4 for
  the drawing as moved, put the same text on the clipboard, and a table and a column named with
  markup characters appear in them only escaped.
- **AK-17 (v8)** Hand test: the diagram and its PNG of the PostgreSQL reference schema next to the
  reference (OBS-0068 E4 again); the two files open in yEd and draw.io with boxes, columns, lines and
  crow's feet in place.

## Out of scope
Opening one diagram for several selected tables (the database tree selects one row), column,
key and comment toggles, other layout styles, curved or diagonal lines, moving lines by hand,
saving moved positions across sessions, toggles that hide columns or keys, routines and other
objects, editing the schema from the diagram, printing, export formats beyond those of behaviour 11.

## Known deviations from the reference
- The diagram is fitted to the view when it opens; the reference opens it cut off (IMP-0053).
- Exports do not depend on what the window shows and include view dependencies; image export offers
  SVG and PNG with a scale; there is no web-editor target (IMP-0053, IMP-0054).
- Columns stay in definition order (the reference puts foreign keys first and the primary key last).
- Lines go around boxes they would cross (v8, behaviour 16); whether the reference does is not
  compared yet (OBS-0068 E4). Moved positions are not saved.
- yEd and draw.io exports are files and clipboard text only, like the others; nothing opens a web
  editor (IMP-0054).
- Column labels of lines that share a gap between two layers can touch each other; the layers move
  apart for long labels (SPEC-0022 v3 behaviour 14) but lines are not re-routed (v5).
- Which keys select boxes and open the export menu in the reference is still to be compared; the
  keys of behaviour 13 are our own.
