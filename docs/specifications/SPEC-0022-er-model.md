# SPEC-0022: Entity-relationship model

- **Version:** 4 · **Date:** 2026-11-16 (v4: yEd GraphML and draw.io exports, names made safe for XML —
  OBS-0068 L10); 2026-11-14 (v1 2026-10-31; v2: views and their dependencies, PlantUML,
  SVG and PNG, exports always complete, no uploads — IMP-0053, IMP-0054; v3: foreign-key lines are
  labelled with their columns)
- **Sources:** GEN (database tools draw tables with their columns and keys and connect them along foreign
  keys; F-056), STD (Mermaid `erDiagram` syntax, PlantUML entity diagrams, Graphviz DOT language, SVG 1.1,
  PNG; v4: XML 1.0, GraphML 1.0 with the published yWorks extension that yEd reads, the published
  `.drawio` file format of draw.io, an mxGraph model in an `mxfile`), OBS (OBS-0010, A1; OBS-0068 L10),
  SPEC-0013 (schema model)
- **Feature matrix rows:** F-056

## Purpose
Turn a schema into a diagram model (entities, relationships with cardinality, positions) that the UI
draws and that can be exported as text, so diagrams can be versioned and reviewed like code.

## Behaviour
1. Input: schema (SPEC-0013), optional table names and optional neighbour depth. Without names all tables
   are included; with names, tables reachable over up to `depth` foreign keys (either direction) are
   added. Unknown names are an error. Views are included as entities of kind `view` unless the caller
   leaves them out; with a table selection a view is included when it uses a selected table. [GEN]
2. Entities list columns in definition order with type, nullability and key marks: `PK` (primary key),
   `FK` (in a foreign key), `UK` (in a unique index that is not the primary key). [GEN]
3. A relationship per foreign key, from the referencing table to the referenced one, with the columns.
   Cardinality on the referencing side is "many", or "one" when the foreign-key columns are the primary
   key or a unique index; on the referenced side "exactly one", or "zero or one" when a foreign-key
   column is nullable. Foreign keys to tables outside the selection are listed separately, not dropped
   silently. [GEN]
4. Layout: tables are placed in layers so that referenced tables are left of the tables that reference
   them (layer = longest chain of foreign keys to a table without outgoing ones; cycles and
   self-references do not loop); within a layer by name. Positions are grid cells (layer, row). The layout
   is deterministic. [GEN]
5. Mermaid output: an `erDiagram` with one block per entity and one line per relationship; names and
   types that Mermaid does not accept are made safe (other characters become `_`) and the original is kept
   as a comment. DOT output: one HTML-table node per entity and one edge per relationship, with `&`, `<`,
   `>` and `"` escaped. [STD]
6. MCP tool `er_diagram` (connection, optional tables and depth, format `mermaid` | `plantuml` | `dot` |
   `svg` | `png` | `json`, optional `scale` and `path`): needs `read-schema`. `png` is written to `path`
   inside the workspace (required) and the result names the file and its pixel size. **v4:** formats
   `graphml` and `drawio` return the documents of behaviours 15 and 16 as text, with the boxes moved by
   `positions` as for `svg`. [ADR-0008]
7. **View dependencies (v2):** the tables and views a view uses are found in its stored definition
   (SPEC-0013 view definitions): every identifier of the definition outside literals and comments whose
   last part names a table or another view of the schema. Each pair is a dependency from the view to
   what it uses; a view never depends on itself. [GEN]
8. **Exports are complete (v2):** every text and image export writes all entities with all columns in
   definition order, their types, key marks, all relationships with their columns and all view
   dependencies — whatever the window currently shows. [GEN, OBS: IMP-0053]
9. **Mermaid (v2):** views are entities whose block carries a comment line `%% view`; a dependency is a
   dotted (non-identifying) line `table ||..o{ view : "used by view"`. [STD]
10. **PlantUML (v2):** `@startuml` … `@enduml` with `hide circle` and orthogonal lines; one
    `entity "<name>" as <safe name>` per table (`<<view>>` for views) with the primary-key columns above a
    `--` separator, each line `name : type` plus `<<PK>>`, `<<FK>>`, `<<UK>>` and `NOT NULL`; relationships
    in crow's-foot notation (`||--o{`, `|o--o{`, `||--||`) labelled with the referencing columns; view
    dependencies as `view ..> table : uses`. [STD]
11. **SVG (v2):** the drawing of SPEC-0111 (box positions, lines with their elbows, cardinality labels)
    as a standalone SVG document: a white background, boxes with a header band, one text line per
    column (key marks, name, type), views with a dashed border, dependency lines dashed; text uses a
    generic monospace font family and is XML-escaped. The document's size is the canvas size. [STD]
12. **PNG (v2):** the SVG rendered with a scale factor from 1 to 4 (default 2); the image has the
    canvas size times the scale. [STD]
13. **Local only (v2):** every export goes to a local file or the clipboard. No export sends the schema
    to any service; the export code makes no network connection. **v4:** this includes GraphML and
    draw.io: neither is opened in a web editor or anywhere else by the export, and neither file
    points a reader at a host (namespace names are identifiers only; GraphML has no schema
    location). [GEN, OBS: IMP-0054]
14. **Column label of a relationship (v3):** every relationship has a label made of its column
    pairs, joined by `, `: the referencing column alone when the referenced column has the same name
    (`sku`), otherwise `referencing:referenced` (`warehouse_id:id`). A relationship without columns
    (a dependency diagram) and a view dependency have none. The drawing (format `layout`, SVG and
    PNG) carries the label and the point it is centred on: the middle of the line's middle segment.
    Layers are at least as far apart as the longest label of the diagram needs, so a label does not
    run into the boxes of its two layers. [GEN, OBS: OBS-0010 A1]
15. **yEd GraphML (v4):** a GraphML 1.0 document (namespace `http://graphml.graphdrawing.org/xmlns`)
    with the yWorks extension (namespace `http://www.yworks.com/xml/graphml`) that the yEd editor
    reads, built from the drawing of SPEC-0111 (boxes as moved, lines with their routes): a key for
    node graphics, one for edge graphics, and plain `name` (node) and `columns` (edge) keys for other
    GraphML readers. One node per entity, a generic entity-relationship node (configuration
    `com.yworks.entityRelationship.big_entity`) at the place and size of its box, with a name label
    (the name, ` (view)` after a view's) and an attributes label holding one line per column in
    definition order (`keys name : type`, keys left out when there are none); a view has a dashed
    border. One edge per relationship from the referenced to the referencing entity: the line's
    inner points as bends, its end points as ports (relative to the box centres), the column label
    of behaviour 14 as edge label, and crow's feet: `crows_foot_one_mandatory` or
    `crows_foot_one_optional` at the referenced end (exactly one, zero or one) and
    `crows_foot_many_optional` or `crows_foot_one_optional` at the referencing end (many, one). A
    relationship without columns has plain ends. One dashed edge without arrows per view dependency,
    from what the view uses to the view. Nodes and edges are numbered (`n0`, `e0`) in the order of
    the drawing. [STD]
16. **draw.io (v4):** an uncompressed `.drawio` document: `mxfile` with one `diagram` holding an
    `mxGraphModel` with the two root cells. Per entity a container cell (style `swimlane`, its rows
    stacked) at the place and size of its box, value the name (` (view)` after a view's, border
    dashed), with one child cell per column in definition order (`keys name : type`) at its row. Per
    relationship an edge cell from the referenced to the referencing entity, value the column label,
    with ER markers (`ERmandOne` or `ERzeroToOne` at the referenced end; `ERzeroToMany` or
    `ERzeroToOne` at the referencing end), its end points as fixed exit and entry points of the two
    boxes and the inner points of the line as waypoints, so the editor draws the route of the window;
    view dependencies dashed without markers. Cells are numbered (`t0`, `t0c1`, `r0`). [STD]
17. **Names in XML (v4):** in SVG, GraphML and draw.io every name, type and label is written as XML
    text: `&`, `<`, `>`, `"` and `'` as entities, line feed, carriage return and tab as character
    references, and characters that XML 1.0 does not allow (control characters, U+FFFE, U+FFFF) as
    U+FFFD. Names never become ids, element or attribute names, or style values. Labels are plain
    text: the draw.io cells never switch on HTML labels (`html=1`), and a GraphML label that would
    begin with `<` gets a zero-width space in front, so yEd does not read it as an HTML label (which
    could load images from the network). A table or column name can therefore not add elements,
    attributes, styles or HTML to the file. [STD, GEN]

## Acceptance criteria
- **AK-1** Key marks, relationship cardinalities (many/one, exactly one/zero or one) and outside
  references for a hand-made schema.
- **AK-2** Selection with depth 0, 1, 2 and unknown names.
- **AK-3** Layout layers for a chain, a diamond, a cycle and a self-reference.
- **AK-4** Exact Mermaid and DOT output for a small schema, including made-safe names and escaping.
- **AK-5** The SQLite reference schema gives entities `item`, `movement`, `stock`, `warehouse` and the four
  relationships `stock`/`movement` → `warehouse`/`item`, with `warehouse` and `item` in layer 0.
- **AK-6** The MCP tool returns all formats, writes PNG only inside the workspace and checks `read-schema`.
- **AK-7 (v2)** View dependencies from definitions with qualified, quoted and aliased names, comments
  and literals; a view on a view.
- **AK-8 (v2)** Golden files: the Mermaid and PlantUML exports of the `sql-basic` schema (tables, keys,
  four foreign keys, the view `stock_value` using `stock` and `item`) match
  `crates/database/tests/golden/sql-basic.{mmd,puml}`.
- **AK-9 (v2)** SVG is well-formed XML with one group per entity and one path per line, escapes `<&"`;
  PNG decodes with the scaled size.
- **AK-10 (v2)** The PostgreSQL reference schema (live) gives the view `stock_value` with dependencies on
  `stock` and `item`.

- **AK-11 (v3)** Labels for equal names, different names and two column pairs; none for a plain
  edge and a dependency; the label point is the middle of the middle segment and follows a moved
  box; a 40-character label widens the gap between its layers; the SVG holds the label, escaped and
  centred (`er_layout::tests::relationship_lines_name_their_columns`).
- **AK-12 (v4, behaviour 15)** GraphML of a schema with a table, a referencing table, a view and its
  dependency (`er_editors::tests::graphml_holds_boxes_columns_and_crows_feet`): well-formed XML in the
  two namespaces, without a schema location; one node per box with the box's geometry, name and attribute labels with every
  column in definition order; one edge per line from the referenced to the referencing node with
  the line's inner points as bends, ports at its end points, the column label and the crow's feet
  for exactly one / zero or one / many / one; the dependency dashed without arrows.
- **AK-13 (v4, behaviour 16)** draw.io of the same schema
  (`er_editors::tests::drawio_holds_boxes_columns_and_er_markers`): well-formed XML, `mxfile` →
  `diagram` → `mxGraphModel` → `root` with cells `0` and `1`; one swimlane per box at its geometry
  with one child cell per column; one edge per line with source, target, ER markers, exit and entry
  points that reproduce the line's end points, its inner points as waypoints; no cell has `html=1`.
- **AK-14 (v4, behaviour 17)** Names with markup (`</y:NodeLabel><y:Evil/>`, `"/><mxCell id="x`,
  `<html><img src="http://example.invalid/x.png">`, `a&b'c`, a control character and a line break)
  as table, column and view names: SVG, GraphML and draw.io stay well-formed, have exactly the
  elements and cells of the model, hold each name as text, unchanged but for U+FFFD in place of the
  control character, no GraphML label starts with `<` and no style holds a name
  (`er_editors::tests::names_cannot_add_markup`).
- **AK-15 (v4, behaviours 6, 13)** MCP `er_diagram` with `graphml` and `drawio` on SQLite with a
  table named with markup characters, also with `positions` (protocol test
  `er_diagram_editor_formats`); the export code of all formats has no network call (review:
  `er_editors.rs` uses only string building).

## Out of scope
Drawing and interaction (UI, SPEC-0111), notes, editor formats other than yEd GraphML and draw.io
(v4), compressed draw.io files, printing.

## Known deviations from the reference
- Text exports are entity-relationship diagrams (Mermaid `erDiagram`, PlantUML entities) with columns,
  types, keys, foreign keys and view dependencies, independent of what the window shows; the reference
  emits class diagrams that follow the current view toggles and drop view dependencies (IMP-0053).
- Image export offers SVG and PNG with a scale factor (IMP-0053).
- There is no export to a web editor; nothing is uploaded (IMP-0054).
- Listing references to tables outside the selection is an addition.
- yEd and draw.io files carry the whole model with the boxes and line routes of the window, written
  locally in one step; nothing is opened in a web editor (IMP-0053, IMP-0054).
