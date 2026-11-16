//! Geometry of the ER diagram (`SPEC-0111`): where each table's box goes, how big it is, and the
//! points and labels of each relationship's line. Pixels, origin at the top left of the canvas.
//! v2: boxes can be moved and the lines follow, zoom steps and fit, SVG and PNG
//! (`SPEC-0022` v2 behaviours 11–12). v8: a line whose plain route would cross another box goes
//! around it ([`crate::er_route`], `SPEC-0111` behaviour 16); names in the SVG are XML text
//! (`SPEC-0022` v4 behaviour 17).

use std::collections::BTreeMap;

use crate::er_route::{self, CLEARANCE, Port, Rect};
use crate::{Entity, ErModel, Relationship};

/// Width of one character of the diagram font.
pub const CHAR_WIDTH: f32 = 7.2;
/// Height of a column line.
pub const LINE_HEIGHT: f32 = 20.;
/// Height of a box's header.
pub const HEADER_HEIGHT: f32 = 26.;
/// Narrowest and widest box.
pub const MIN_WIDTH: f32 = 160.;
/// Widest box; longer names are cut with `…` (v3: wide enough for `timestamp without time zone`).
pub const MAX_WIDTH: f32 = 480.;
/// Space left and right of the text in a box, and between its parts.
pub const INSET: f32 = 8.;
/// Narrowest key column (room for `PK FK`).
pub const KEY_MIN_WIDTH: f32 = 44.;
/// Space between layers, room for the lines.
const LAYER_GAP: f32 = 120.;
/// Width of one character of a line's column label (the small text of the cardinalities).
pub const LABEL_CHAR_WIDTH: f32 = 6.;
/// Space a column label keeps to the boxes on both sides of its gap.
const LABEL_INSET: f32 = 12.;
/// Space between boxes of one layer.
const ROW_GAP: f32 = 40.;
/// Space around the diagram.
pub const MARGIN: f32 = 24.;
/// How far a self-reference loops out of its box.
const LOOP: f32 = 28.;
/// Distance between vertical segments of lines that would otherwise lie on each other (v6 behaviour
/// 15).
const LANE: f32 = 8.;
/// Smallest zoom.
pub const ZOOM_MIN: f32 = 0.25;
/// Largest zoom.
pub const ZOOM_MAX: f32 = 4.;
/// Factor of one zoom step.
pub const ZOOM_STEP: f32 = 1.25;

/// A table's box.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BoxLayout {
    /// Table name.
    pub name: String,
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height: header and one line per column.
    pub height: f32,
    /// Column lines: key marks, name, type, and whether the type is nullable.
    pub lines: Vec<(String, String, String, bool)>,
    /// Width of the key column; names start after it (v3).
    pub key_width: f32,
    /// A view (dashed border).
    pub view: bool,
}

impl BoxLayout {
    /// Vertical middle of column line `i`, or of the header when there is no such column.
    fn row_y(&self, column: Option<usize>) -> f32 {
        match column {
            Some(i) => self.y + HEADER_HEIGHT + LINE_HEIGHT * (i as f32 + 0.5),
            None => self.y + HEADER_HEIGHT / 2.,
        }
    }

    fn right(&self) -> f32 {
        self.x + self.width
    }

    /// The box's rectangle.
    pub fn rect(&self) -> Rect {
        Rect {
            x0: self.x,
            y0: self.y,
            x1: self.right(),
            y1: self.y + self.height,
        }
    }
}

/// A relationship's line from the referenced table to the referencing one, or a view dependency's
/// line from the used table to the view.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LineLayout {
    /// Referencing table (or the view).
    pub from: String,
    /// Referenced table (or what the view uses).
    pub to: String,
    /// Polyline points, starting at the referenced table.
    pub points: Vec<(f32, f32)>,
    /// Label at the referenced end: `1` or `0..1` (empty for a dependency).
    pub to_label: String,
    /// Label at the referencing end: `N` or `1` (empty for a dependency).
    pub from_label: String,
    /// The relationship's columns (`SPEC-0022` v3 behaviour 14); empty for a plain edge and a
    /// dependency.
    pub label: String,
    /// The point the column label is centred on: the middle of the middle segment.
    pub label_at: (f32, f32),
    /// A view dependency (dashed).
    pub dependency: bool,
}

/// The whole drawing.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DiagramLayout {
    /// Boxes in entity order.
    pub boxes: Vec<BoxLayout>,
    /// One line per relationship, then one per view dependency.
    pub lines: Vec<LineLayout>,
    /// Size of the canvas that holds everything.
    pub width: f32,
    /// Height of the canvas.
    pub height: f32,
}

/// The column label of a relationship (`SPEC-0022` v3 behaviour 14): per column pair the
/// referencing column, with `:` and the referenced one when the names differ; pairs joined by
/// `, `. Empty without columns.
pub fn relationship_label(r: &Relationship) -> String {
    r.from_columns
        .iter()
        .enumerate()
        .map(|(i, from)| match r.to_columns.get(i) {
            Some(to) if to != from => format!("{from}:{to}"),
            _ => from.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The middle of the middle segment of a polyline.
fn middle(points: &[(f32, f32)]) -> (f32, f32) {
    match points.len() {
        0 => (0., 0.),
        n => {
            let (a, b) = (points[(n - 1) / 2], points[n / 2]);
            ((a.0 + b.0) / 2., (a.1 + b.1) / 2.)
        }
    }
}

fn box_lines(e: &Entity) -> Vec<(String, String, String, bool)> {
    e.columns
        .iter()
        .map(|c| {
            (
                c.keys.join(" "),
                c.name.clone(),
                c.data_type.clone(),
                c.nullable,
            )
        })
        .collect()
}

/// Width of `n` characters of the diagram font, with half a character to spare for fonts a little
/// wider than the nominal one.
fn chars(n: usize) -> f32 {
    if n == 0 {
        0.
    } else {
        (n as f32 + 0.5) * CHAR_WIDTH
    }
}

fn key_width(e: &Entity) -> f32 {
    let longest = e
        .columns
        .iter()
        .map(|c| c.keys.join(" ").chars().count())
        .max()
        .unwrap_or(0);
    chars(longest).max(KEY_MIN_WIDTH).ceil()
}

/// The width a box needs to show its header and every line whole, as the window and the SVG draw
/// them: inset, keys, gap, name, gap, type, inset (`SPEC-0111` v3, D43).
fn box_width(e: &Entity) -> f32 {
    let header = INSET + chars(e.name.chars().count() + if e.view { 7 } else { 0 }) + INSET;
    let keys = key_width(e);
    let longest = e
        .columns
        .iter()
        .map(|c| {
            INSET
                + keys
                + INSET
                + chars(c.name.chars().count())
                + 2. * INSET
                + chars(c.data_type.chars().count())
                + INSET
        })
        .fold(header, f32::max);
    longest.ceil().clamp(MIN_WIDTH, MAX_WIDTH)
}

/// `name` as it fits line `ty` of box `b`: whole, or cut with `…` when the box is at its widest.
pub fn fitted_name(b: &BoxLayout, name: &str, ty: &str) -> String {
    let room =
        b.width - INSET - b.key_width - INSET - 2. * INSET - chars(ty.chars().count()) - INSET;
    let fits = (room / CHAR_WIDTH).floor().max(1.) as usize;
    let n = name.chars().count();
    if n <= fits {
        name.to_owned()
    } else {
        let mut cut: String = name.chars().take(fits.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

/// Lays out `model`: layers as columns as wide as their widest box, rows top to bottom.
pub fn layout(model: &ErModel) -> DiagramLayout {
    let layers = model
        .entities
        .iter()
        .map(|e| e.layer + 1)
        .max()
        .unwrap_or(0);
    let mut layer_width = vec![0f32; layers];
    for e in &model.entities {
        layer_width[e.layer] = layer_width[e.layer].max(box_width(e));
    }
    // Layers move apart for the longest column label, so it does not run into the boxes on
    // either side of its gap (SPEC-0022 v3 behaviour 14).
    let gap = model
        .relationships
        .iter()
        .map(|r| chars(relationship_label(r).chars().count()) / CHAR_WIDTH * LABEL_CHAR_WIDTH)
        .fold(LAYER_GAP, |gap, label| gap.max(label + 2. * LABEL_INSET));
    let mut layer_x = Vec::with_capacity(layers);
    let mut x = MARGIN;
    for w in &layer_width {
        layer_x.push(x);
        x += w + gap;
    }
    let mut order: Vec<&Entity> = model.entities.iter().collect();
    order.sort_by_key(|e| (e.layer, e.row));
    let mut next_y = vec![MARGIN; layers];
    let mut placed: Vec<(String, BoxLayout)> = Vec::new();
    for e in order {
        let height = HEADER_HEIGHT + LINE_HEIGHT * e.columns.len() as f32;
        let b = BoxLayout {
            name: e.name.clone(),
            x: layer_x[e.layer],
            y: next_y[e.layer],
            width: box_width(e),
            height,
            lines: box_lines(e),
            key_width: key_width(e),
            view: e.view,
        };
        next_y[e.layer] += height + ROW_GAP;
        placed.push((e.name.clone(), b));
    }
    let boxes: Vec<BoxLayout> = model
        .entities
        .iter()
        .filter_map(|e| {
            placed
                .iter()
                .find(|(n, _)| *n == e.name)
                .map(|(_, b)| b.clone())
        })
        .collect();
    route(model, boxes)
}

/// The layout with the boxes named in `positions` moved to their (x, y) (MCP `positions`).
pub fn layout_with_positions(
    model: &ErModel,
    positions: &BTreeMap<String, (f32, f32)>,
) -> DiagramLayout {
    let mut boxes = layout(model).boxes;
    for b in &mut boxes {
        if let Some(&(x, y)) = positions.get(&b.name) {
            b.x = x.max(MARGIN);
            b.y = y.max(MARGIN);
        }
    }
    route(model, boxes)
}

/// Lines and canvas size for `boxes` as they stand (v2 behaviour 9).
pub fn route(model: &ErModel, boxes: Vec<BoxLayout>) -> DiagramLayout {
    let mut lines: Vec<LineLayout> = model
        .relationships
        .iter()
        .filter_map(|r| line(r, model, &boxes))
        .collect();
    let find = |name: &str| boxes.iter().find(|b| b.name == name);
    for d in &model.dependencies {
        let (Some(used), Some(view)) = (find(&d.uses), find(&d.view)) else {
            continue;
        };
        let points = connect(used, used.row_y(None), view, view.row_y(None));
        lines.push(LineLayout {
            from: d.view.clone(),
            to: d.uses.clone(),
            label_at: middle(&points),
            points,
            to_label: String::new(),
            from_label: String::new(),
            label: String::new(),
            dependency: true,
        });
    }
    let rects: Vec<Rect> = boxes.iter().map(BoxLayout::rect).collect();
    for l in &mut lines {
        lead_around(l, &boxes, &rects);
    }
    separate_horizontals(&mut lines, &boxes, &rects);
    separate_verticals(&mut lines, &boxes, &rects);
    let right = boxes
        .iter()
        .map(BoxLayout::right)
        .chain(lines.iter().flat_map(|l| l.points.iter().map(|p| p.0)))
        .fold(0f32, f32::max);
    let bottom = boxes
        .iter()
        .map(|b| b.y + b.height)
        .chain(lines.iter().flat_map(|l| l.points.iter().map(|p| p.1)))
        .fold(0f32, f32::max);
    DiagramLayout {
        boxes,
        lines,
        width: right + MARGIN,
        height: bottom + MARGIN,
    }
}

/// The indices of the boxes a line starts at (what is referenced or used) and ends at.
fn ends(line: &LineLayout, boxes: &[BoxLayout]) -> Option<(usize, usize)> {
    let index = |name: &str| boxes.iter().position(|b| b.name == name);
    index(&line.to).zip(index(&line.from))
}

/// Replaces the route of `line` by one around the boxes in its way when it comes closer than
/// [`CLEARANCE`] to a box that is not one of its ends, or to its own boxes away from its end points
/// (`SPEC-0111` v8 behaviour 16: running along a box counts like crossing it). It leaves and enters
/// its boxes where it did; without a way out (an end point covered by another box) it keeps its
/// route.
fn lead_around(line: &mut LineLayout, boxes: &[BoxLayout], rects: &[Rect]) {
    let Some((start, end)) = ends(line, boxes) else {
        return;
    };
    if er_route::keeps_clear(&line.points, rects, start, end, CLEARANCE) {
        return;
    }
    let port = |at: (f32, f32), b: &BoxLayout| Port {
        at,
        out: if (at.0 - b.right()).abs() < 0.01 {
            1.
        } else {
            -1.
        },
    };
    let (Some(&first), Some(&last)) = (line.points.first(), line.points.last()) else {
        return;
    };
    if let Some(points) = er_route::around(
        port(first, &boxes[start]),
        port(last, &boxes[end]),
        rects,
        [start, end],
    ) {
        line.label_at = middle(&points);
        line.points = points;
    }
}

/// Moves each inner vertical segment of each line aside, in steps of [`LANE`], while it would lie on
/// one of an earlier line over a common height (`SPEC-0111` v6 behaviour 15): an elbow between its
/// neighbours stays between them, a segment with both neighbours on one side only moves away from
/// them. v8: a segment only moves where the line stays [`CLEARANCE`] away from every box (behaviour
/// 16). Where no lane is free the segment keeps its place.
fn separate_verticals(lines: &mut [LineLayout], boxes: &[BoxLayout], rects: &[Rect]) {
    separate(lines, boxes, rects, true);
}

/// [`separate_verticals`] for the inner horizontal segments of routes around boxes (v8 behaviour
/// 16): two routes through the same gap between boxes do not run on one another.
fn separate_horizontals(lines: &mut [LineLayout], boxes: &[BoxLayout], rects: &[Rect]) {
    separate(lines, boxes, rects, false);
}

/// The lane pass of one direction: `vertical` segments keep their x apart, the others their y.
fn separate(lines: &mut [LineLayout], boxes: &[BoxLayout], rects: &[Rect], vertical: bool) {
    // The coordinate a segment of this direction keeps, and the one it runs along.
    let at = |p: (f32, f32)| if vertical { p.0 } else { p.1 };
    let along = |p: (f32, f32)| if vertical { p.1 } else { p.0 };
    let set = |p: &mut (f32, f32), v: f32| {
        if vertical {
            p.0 = v;
        } else {
            p.1 = v;
        }
    };
    let mut placed: Vec<(f32, f32, f32)> = Vec::new();
    for line in lines.iter_mut() {
        let own = ends(line, boxes);
        let mut moved = false;
        for k in 1..line.points.len().saturating_sub(2) {
            let p = &line.points;
            // A horizontal of no length is the straight line of facing rows: it stays.
            if at(p[k]) != at(p[k + 1]) || (!vertical && along(p[k]) == along(p[k + 1])) {
                continue;
            }
            let x = at(p[k]);
            let (y0, y1) = (
                along(p[k]).min(along(p[k + 1])),
                along(p[k]).max(along(p[k + 1])),
            );
            let (before, after) = (at(p[k - 1]), at(p[k + 2]));
            let between = (before < x) != (after < x);
            let (low, high) = if between {
                (before.min(after) + LANE, before.max(after) - LANE)
            } else if before < x {
                (x, f32::MAX)
            } else {
                (f32::MIN, x)
            };
            let clear = |c: f32| {
                let Some((start, end)) = own else {
                    return true;
                };
                let mut shifted = p.clone();
                set(&mut shifted[k], c);
                set(&mut shifted[k + 1], c);
                er_route::keeps_clear_between(&shifted, k - 1..k + 2, rects, start, end, CLEARANCE)
            };
            let free = |c: f32| {
                (low..=high).contains(&c)
                    && !placed
                        .iter()
                        .any(|&(px, py0, py1)| (px - c).abs() < LANE / 2. && py0 <= y1 && y0 <= py1)
                    && (c == x || clear(c))
            };
            let steps = (1..=24).flat_map(|k| {
                let d = ((k + 1) / 2) as f32 * LANE;
                if k % 2 == 1 { [x + d] } else { [x - d] }
            });
            let Some(chosen) = std::iter::once(x).chain(steps).find(|c| free(*c)) else {
                placed.push((x, y0, y1));
                continue;
            };
            if chosen != x {
                set(&mut line.points[k], chosen);
                set(&mut line.points[k + 1], chosen);
                moved = true;
            }
            placed.push((chosen, y0, y1));
        }
        if moved {
            line.label_at = middle(&line.points);
        }
    }
}

/// A polyline from `a` (at height `ay`) to `b` (at `by`): from the side of each box that faces the
/// other, with an elbow halfway; boxes that overlap horizontally are joined around their right sides.
fn connect(a: &BoxLayout, ay: f32, b: &BoxLayout, by: f32) -> Vec<(f32, f32)> {
    if b.x >= a.right() {
        let (start, end) = ((a.right(), ay), (b.x, by));
        let mid = (start.0 + end.0) / 2.;
        vec![start, (mid, start.1), (mid, end.1), end]
    } else if b.right() <= a.x {
        let (start, end) = ((a.x, ay), (b.right(), by));
        let mid = (start.0 + end.0) / 2.;
        vec![start, (mid, start.1), (mid, end.1), end]
    } else {
        let out = a.right().max(b.right()) + LOOP;
        vec![(a.right(), ay), (out, ay), (out, by), (b.right(), by)]
    }
}

fn line(r: &Relationship, model: &ErModel, boxes: &[BoxLayout]) -> Option<LineLayout> {
    let find = |name: &str| boxes.iter().find(|b| b.name == name);
    let column = |table: &str, cols: &[String]| {
        let first = cols.first()?;
        model
            .entities
            .iter()
            .find(|e| e.name == table)?
            .columns
            .iter()
            .position(|c| &c.name == first)
    };
    let to = find(&r.to)?;
    let from = find(&r.from)?;
    let plain = r.from_columns.is_empty() && r.to_columns.is_empty();
    let start_y = to.row_y(column(&r.to, &r.to_columns));
    let end_y = from.row_y(column(&r.from, &r.from_columns));
    let points = if r.from == r.to {
        let out = to.right() + LOOP;
        vec![
            (to.right(), start_y),
            (out, start_y),
            (out, end_y),
            (to.right(), end_y),
        ]
    } else {
        connect(to, start_y, from, end_y)
    };
    Some(LineLayout {
        from: r.from.clone(),
        to: r.to.clone(),
        label: relationship_label(r),
        label_at: middle(&points),
        points,
        // A relationship without columns is a plain edge (a dependency diagram, SPEC-0100 v4
        // behaviour 15): no cardinalities.
        to_label: if plain {
            ""
        } else if r.optional {
            "0..1"
        } else {
            "1"
        }
        .into(),
        from_label: if plain {
            ""
        } else if r.many {
            "N"
        } else {
            "1"
        }
        .into(),
        dependency: false,
    })
}

/// Moves box `i` so its top left corner is at (`x`, `y`), kept inside the top and left margin, and
/// re-routes the lines (v2 behaviour 9). Unknown boxes change nothing.
pub fn move_box(layout: &mut DiagramLayout, model: &ErModel, i: usize, x: f32, y: f32) {
    let Some(b) = layout.boxes.get_mut(i) else {
        return;
    };
    b.x = x.max(MARGIN);
    b.y = y.max(MARGIN);
    *layout = route(model, std::mem::take(&mut layout.boxes));
}

/// The box at canvas point (`x`, `y`), the topmost (last drawn) first.
pub fn box_at(layout: &DiagramLayout, x: f32, y: f32) -> Option<usize> {
    layout
        .boxes
        .iter()
        .rposition(|b| x >= b.x && x <= b.right() && y >= b.y && y <= b.y + b.height)
}

/// One zoom step in (v2 behaviour 7).
pub fn zoom_in(zoom: f32) -> f32 {
    (zoom * ZOOM_STEP).min(ZOOM_MAX)
}

/// One zoom step out.
pub fn zoom_out(zoom: f32) -> f32 {
    (zoom / ZOOM_STEP).max(ZOOM_MIN)
}

/// The largest zoom (at most 1) at which a `width` × `height` canvas fits a viewport (v2
/// behaviour 8); 1 when the viewport is unknown.
pub fn fit_zoom(width: f32, height: f32, viewport_width: f32, viewport_height: f32) -> f32 {
    if viewport_width <= 0. || viewport_height <= 0. || width <= 0. || height <= 0. {
        return 1.;
    }
    (viewport_width / width)
        .min(viewport_height / height)
        .clamp(ZOOM_MIN, 1.)
}

fn xml(text: &str) -> String {
    crate::er_editors::xml_text(text)
}

/// The drawing as a standalone SVG document (`SPEC-0022` v2 behaviour 11).
pub fn to_svg(layout: &DiagramLayout) -> String {
    let n = |v: f32| format!("{}", (v * 10.).round() / 10.);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"monospace\" font-size=\"12\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n",
        w = n(layout.width),
        h = n(layout.height)
    );
    for l in &layout.lines {
        let d: Vec<String> = l
            .points
            .iter()
            .enumerate()
            .map(|(k, (x, y))| format!("{}{} {}", if k == 0 { "M" } else { "L" }, n(*x), n(*y)))
            .collect();
        out.push_str(&format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"#6b7280\" stroke-width=\"1.2\"{}/>\n",
            d.join(" "),
            if l.dependency {
                " stroke-dasharray=\"5 4\""
            } else {
                ""
            }
        ));
        if let (Some(first), Some(last)) = (l.points.first(), l.points.last()) {
            let right_of = |p: &(f32, f32), q: Option<&(f32, f32)>| q.is_some_and(|q| q.0 >= p.0);
            for (p, toward, text) in [
                (first, l.points.get(1), &l.to_label),
                (last, l.points.iter().rev().nth(1), &l.from_label),
            ] {
                if text.is_empty() {
                    continue;
                }
                let x = if right_of(p, toward) {
                    p.0 + 4.
                } else {
                    p.0 - 22.
                };
                out.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" fill=\"#6b7280\" font-size=\"10\">{}</text>\n",
                    n(x),
                    n(p.1 - 4.),
                    xml(text)
                ));
            }
        }
    }
    // Column labels after every line, with a white edge, so no line strikes one through
    // (SPEC-0022 v3 behaviour 14).
    for l in layout.lines.iter().filter(|l| !l.label.is_empty()) {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"#374151\" font-size=\"10\" stroke=\"#ffffff\" stroke-width=\"3\" paint-order=\"stroke\" data-label=\"{}\">{}</text>\n",
            n(l.label_at.0),
            n(l.label_at.1 + 3.5),
            xml(&format!("{}-{}", l.from, l.to)),
            xml(&l.label)
        ));
    }
    for b in &layout.boxes {
        out.push_str(&format!("<g data-table=\"{}\">\n", xml(&b.name)));
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" fill=\"#f9fafb\" stroke=\"#9ca3af\"{}/>\n",
            n(b.x),
            n(b.y),
            n(b.width),
            n(b.height),
            if b.view { " stroke-dasharray=\"5 4\"" } else { "" }
        ));
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" fill=\"#e5e7eb\"/>\n",
            n(b.x + 0.5),
            n(b.y + 0.5),
            n(b.width - 1.),
            n(HEADER_HEIGHT - 1.)
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-weight=\"bold\" fill=\"#111827\">{}{}</text>\n",
            n(b.x + 8.),
            n(b.y + HEADER_HEIGHT / 2. + 4.),
            xml(&b.name),
            if b.view { " (view)" } else { "" }
        ));
        for (i, (keys, name, ty, nullable)) in b.lines.iter().enumerate() {
            let y = b.y + HEADER_HEIGHT + LINE_HEIGHT * (i as f32 + 0.5) + 4.;
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" fill=\"#6b7280\" font-size=\"10\">{}</text>\n",
                n(b.x + 8.),
                n(y),
                xml(keys)
            ));
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" fill=\"#111827\">{}</text>\n",
                n(b.x + INSET + b.key_width + INSET),
                n(y),
                xml(&fitted_name(b, name, ty))
            ));
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" fill=\"{}\">{}</text>\n",
                n(b.right() - INSET),
                n(y),
                if *nullable { "#9ca3af" } else { "#4b5563" },
                xml(ty)
            ));
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// The drawing as PNG at `scale` (1–4) times the canvas size (`SPEC-0022` v2 behaviour 12).
pub fn to_png(layout: &DiagramLayout, scale: f32) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    let scale = scale.clamp(1., 4.);
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(&to_svg(layout), &options).map_err(|e| e.to_string())?;
    let size = tree
        .size()
        .to_int_size()
        .scale_by(scale)
        .ok_or("empty diagram")?;
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("diagram too large")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dependency, ErColumn};

    fn col(name: &str, keys: &[&str], nullable: bool) -> ErColumn {
        ErColumn {
            name: name.into(),
            data_type: "INTEGER".into(),
            nullable,
            keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        }
    }

    fn entity(name: &str, layer: usize, row: usize, columns: Vec<ErColumn>) -> Entity {
        Entity {
            name: name.into(),
            columns,
            layer,
            row,
            view: false,
        }
    }

    fn rel(from: &str, from_col: &str, to: &str, many: bool, optional: bool) -> Relationship {
        Relationship {
            from: from.into(),
            from_columns: vec![from_col.into()],
            to: to.into(),
            to_columns: vec!["id".into()],
            many,
            optional,
        }
    }

    fn model() -> ErModel {
        ErModel {
            entities: vec![
                entity(
                    "orders",
                    1,
                    0,
                    vec![
                        col("id", &["PK"], false),
                        col("customer_id", &["FK"], false),
                        col("parent_id", &["FK"], true),
                    ],
                ),
                entity("customers", 0, 0, vec![col("id", &["PK"], false)]),
                entity(
                    "warehouse",
                    0,
                    1,
                    vec![col("id", &["PK"], false), col("name", &[], true)],
                ),
            ],
            relationships: vec![
                rel("orders", "customer_id", "customers", true, false),
                rel("orders", "parent_id", "orders", true, true),
            ],
            outside: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn boxes_sit_in_layer_columns_and_rows() {
        let l = layout(&model());
        let by = |n: &str| l.boxes.iter().find(|b| b.name == n).unwrap();
        let (customers, warehouse, orders) = (by("customers"), by("warehouse"), by("orders"));
        assert_eq!(l.boxes[0].name, "orders", "boxes keep the entity order");
        assert_eq!((customers.x, customers.y), (MARGIN, MARGIN));
        assert_eq!(warehouse.x, MARGIN);
        assert_eq!(warehouse.y, MARGIN + customers.height + ROW_GAP);
        assert_eq!(customers.height, HEADER_HEIGHT + LINE_HEIGHT);
        assert_eq!(orders.height, HEADER_HEIGHT + 3. * LINE_HEIGHT);
        let column0 = customers.width.max(warehouse.width);
        assert_eq!(orders.x, MARGIN + column0 + LAYER_GAP);
        assert_eq!(
            customers.width, MIN_WIDTH,
            "short lines keep the minimum width"
        );
        assert_eq!(
            orders.lines[1],
            ("FK".into(), "customer_id".into(), "INTEGER".into(), false)
        );
        assert_eq!(l.width, orders.x + orders.width + LOOP + MARGIN);
        assert_eq!(l.height, warehouse.y + warehouse.height + MARGIN);
    }

    #[test]
    fn lines_join_the_key_rows_with_an_elbow() {
        let l = layout(&model());
        let by = |n: &str| l.boxes.iter().find(|b| b.name == n).unwrap().clone();
        let (customers, orders) = (by("customers"), by("orders"));
        let fk = &l.lines[0];
        let start = (
            customers.x + customers.width,
            customers.y + HEADER_HEIGHT + LINE_HEIGHT / 2.,
        );
        let end = (orders.x, orders.y + HEADER_HEIGHT + LINE_HEIGHT * 1.5);
        let mid = (start.0 + end.0) / 2.;
        assert_eq!(fk.points, vec![start, (mid, start.1), (mid, end.1), end]);
        assert_eq!((fk.to_label.as_str(), fk.from_label.as_str()), ("1", "N"));
        let own = &l.lines[1];
        let right = orders.x + orders.width;
        assert_eq!(
            own.points[0],
            (right, orders.y + HEADER_HEIGHT + LINE_HEIGHT / 2.)
        );
        assert_eq!(own.points[1].0, right + LOOP);
        assert_eq!(
            own.points[3],
            (right, orders.y + HEADER_HEIGHT + LINE_HEIGHT * 2.5)
        );
        assert_eq!(own.to_label, "0..1");
    }

    /// D43 (hand test 120): name and type never overlap and are never cut for the reference schema's
    /// columns, in the window geometry and in the SVG.
    /// SPEC-0111 v6 AK-13 (OBS-0051 L5): two keys of one table to the same table get vertical
    /// segments of their own; lines over different heights keep the middle.
    #[test]
    fn lines_share_no_vertical_segment() {
        let mut m = model();
        m.entities[0]
            .columns
            .push(col("billing_id", &["FK"], false));
        m.relationships
            .push(rel("orders", "billing_id", "customers", true, false));
        let l = layout(&m);
        let verticals: Vec<(f32, f32, f32)> = l
            .lines
            .iter()
            .filter(|l| l.points.len() == 4 && l.points[1].0 == l.points[2].0)
            .map(|l| {
                let (a, b) = (l.points[1].1, l.points[2].1);
                (l.points[1].0, a.min(b), a.max(b))
            })
            .collect();
        for (i, a) in verticals.iter().enumerate() {
            for b in &verticals[i + 1..] {
                assert!(
                    (a.0 - b.0).abs() >= LANE / 2. || a.2 < b.1 || b.2 < a.1,
                    "{a:?} and {b:?} share a vertical segment"
                );
            }
        }
        // The first line keeps the middle, the second stays inside the gap between the boxes.
        let (first, second) = (&l.lines[0], &l.lines[2]);
        let mid = (first.points[0].0 + first.points[3].0) / 2.;
        assert_eq!(first.points[1].0, mid);
        assert!(second.points[1].0 != mid);
        assert!(second.points[1].0 > second.points[0].0 && second.points[1].0 < second.points[3].0);
    }

    #[test]
    fn boxes_hold_name_and_type_side_by_side() {
        let column = |name: &str, ty: &str, keys: &[&str]| ErColumn {
            name: name.into(),
            data_type: ty.into(),
            nullable: false,
            keys: keys.iter().map(|k| k.to_string()).collect(),
        };
        let model = ErModel {
            entities: vec![
                entity(
                    "item",
                    0,
                    0,
                    vec![
                        column("sku", "text", &["PK"]),
                        column("unit_price_cents", "integer", &[]),
                    ],
                ),
                entity(
                    "movement",
                    0,
                    1,
                    vec![
                        column("id", "integer", &["PK"]),
                        column("warehouse_id", "integer", &["PK", "FK", "UK"]),
                        column("moved_at", "timestamp without time zone", &[]),
                    ],
                ),
            ],
            relationships: vec![],
            outside: vec![],
            dependencies: vec![],
        };
        let l = layout(&model);
        for b in &l.boxes {
            for (keys, name, ty, _) in &b.lines {
                assert!(
                    chars(keys.chars().count()) <= b.key_width,
                    "{keys} in {}",
                    b.name
                );
                assert_eq!(
                    &fitted_name(b, name, ty),
                    name,
                    "{name} is cut in {}",
                    b.name
                );
                let name_end = b.x + INSET + b.key_width + INSET + chars(name.chars().count());
                let type_start = b.x + b.width - INSET - chars(ty.chars().count());
                assert!(
                    name_end + INSET <= type_start,
                    "{name}/{ty} overlap in {}",
                    b.name
                );
            }
        }
        let svg = to_svg(&l);
        assert!(svg.contains(">moved_at<") && svg.contains(">timestamp without time zone<"));
        // At the widest, a name too long is cut with an ellipsis rather than run into the type.
        let long = entity(
            "t",
            0,
            0,
            vec![column(&"n".repeat(80), "timestamp without time zone", &[])],
        );
        let b = &layout(&ErModel {
            entities: vec![long],
            relationships: vec![],
            outside: vec![],
            dependencies: vec![],
        })
        .boxes[0];
        assert_eq!(b.width, MAX_WIDTH);
        let shown = fitted_name(b, &"n".repeat(80), "timestamp without time zone");
        assert!(
            shown.ends_with('…') && shown.chars().count() < 80,
            "{shown}"
        );
    }

    #[test]
    fn long_lines_widen_boxes_up_to_a_limit() {
        let wide = |n: usize| {
            box_width(&entity(
                "t",
                0,
                0,
                vec![ErColumn {
                    name: "c".repeat(n),
                    data_type: "TEXT".into(),
                    nullable: true,
                    keys: vec![],
                }],
            ))
        };
        assert!(wide(30) > MIN_WIDTH);
        assert_eq!(wide(200), MAX_WIDTH);
        let empty = layout(&ErModel {
            entities: vec![],
            relationships: vec![],
            outside: vec![],
            dependencies: vec![],
        });
        assert!(empty.boxes.is_empty() && empty.lines.is_empty());
    }

    /// SPEC-0111/AK-4: zoom steps and limits, fit.
    #[test]
    fn zoom_steps_limits_and_fit() {
        assert_eq!(zoom_in(1.), 1.25);
        assert_eq!(zoom_out(1.25), 1.);
        let mut z = 1.;
        for _ in 0..20 {
            z = zoom_in(z);
        }
        assert_eq!(z, ZOOM_MAX);
        for _ in 0..40 {
            z = zoom_out(z);
        }
        assert_eq!(z, ZOOM_MIN);
        assert_eq!(fit_zoom(400., 300., 800., 600.), 1., "small canvas: 100 %");
        assert_eq!(fit_zoom(1600., 600., 800., 600.), 0.5, "wide canvas");
        assert_eq!(fit_zoom(800., 2400., 800., 600.), 0.25, "tall canvas");
        assert_eq!(
            fit_zoom(8000., 600., 800., 600.),
            ZOOM_MIN,
            "not below the minimum"
        );
        assert_eq!(fit_zoom(800., 600., 0., 0.), 1., "unknown viewport");
    }

    /// SPEC-0111/AK-4: moving a box re-routes its lines and grows the canvas; layout resets.
    #[test]
    fn moving_a_box_reroutes_lines_and_grows_the_canvas() {
        let m = model();
        let mut l = layout(&m);
        let original = l.clone();
        let orders = l.boxes.iter().position(|b| b.name == "orders").unwrap();
        // Move orders left of customers' column and far down: the line now leaves customers'
        // left side... customers stays at the margin, so put orders below and overlapping.
        move_box(&mut l, &m, orders, MARGIN, 600.);
        let b = &l.boxes[orders];
        assert_eq!((b.x, b.y), (MARGIN, 600.));
        let fk = &l.lines[0];
        assert_eq!(
            fk.points[0].0,
            l.boxes[1].x + l.boxes[1].width,
            "around the right side"
        );
        assert_eq!(
            fk.points.last().unwrap().1,
            600. + HEADER_HEIGHT + LINE_HEIGHT * 1.5
        );
        assert!(l.height >= 600. + b.height + MARGIN, "canvas grows");
        // Right of customers again, then far right: the line joins facing sides.
        move_box(&mut l, &m, orders, 900., 40.);
        let fk = &l.lines[0];
        assert_eq!(fk.points.last().unwrap().0, 900.);
        assert_eq!(l.width, 900. + l.boxes[orders].width + LOOP + MARGIN);
        // Left of customers: the line starts at customers' left edge.
        let customers = l.boxes.iter().position(|b| b.name == "customers").unwrap();
        move_box(&mut l, &m, customers, 1400., 40.);
        let fk = &l.lines[0];
        assert_eq!(fk.points[0].0, 1400.);
        assert_eq!(
            fk.points.last().unwrap().0,
            900. + l.boxes[orders].width,
            "ends on orders' right side"
        );
        // Boxes stay inside the margin; unknown boxes are ignored.
        move_box(&mut l, &m, orders, -50., -50.);
        assert_eq!((l.boxes[orders].x, l.boxes[orders].y), (MARGIN, MARGIN));
        move_box(&mut l, &m, 99, 0., 0.);
        assert_eq!(layout(&m), original, "layout puts everything back");
        assert_eq!(box_at(&original, MARGIN + 1., MARGIN + 1.), Some(1));
        assert_eq!(box_at(&original, 0., 0.), None);
        let moved = layout_with_positions(&m, &[("orders".to_owned(), (900., 40.))].into());
        assert_eq!((moved.boxes[orders].x, moved.boxes[orders].y), (900., 40.));
    }

    fn with_view() -> ErModel {
        let mut m = model();
        let mut v = entity("big_orders", 2, 0, vec![col("id", &[], true)]);
        v.view = true;
        m.entities.push(v);
        m.dependencies.push(Dependency {
            view: "big_orders".into(),
            uses: "orders".into(),
        });
        m
    }

    #[test]
    fn view_dependencies_are_dashed_lines_between_headers() {
        let l = layout(&with_view());
        let dep = l.lines.iter().find(|l| l.dependency).unwrap();
        let (orders, view) = (&l.boxes[0], &l.boxes[3]);
        assert!(view.view);
        assert_eq!(
            dep.points[0],
            (orders.right(), orders.y + HEADER_HEIGHT / 2.)
        );
        assert_eq!(
            *dep.points.last().unwrap(),
            (view.x, view.y + HEADER_HEIGHT / 2.)
        );
        assert!(dep.to_label.is_empty() && dep.from_label.is_empty());
    }

    /// SPEC-0022/AK-9: SVG is well-formed, has one group per box and one path per line, escapes.
    /// SPEC-0022/AK-11 (v3).
    #[test]
    fn relationship_lines_name_their_columns() {
        let mut same = rel("stock", "sku", "item", true, false);
        same.to_columns = vec!["sku".into()];
        assert_eq!(relationship_label(&same), "sku");
        let other = rel("orders", "customer_id", "customers", true, false);
        assert_eq!(relationship_label(&other), "customer_id:id");
        let mut two = other.clone();
        two.from_columns.push("region".into());
        two.to_columns.push("region".into());
        assert_eq!(relationship_label(&two), "customer_id:id, region");
        let mut plain = other.clone();
        plain.from_columns.clear();
        plain.to_columns.clear();
        assert_eq!(relationship_label(&plain), "");

        let m = model();
        let l = layout(&m);
        let line = &l.lines[0];
        assert_eq!(line.label, "customer_id:id");
        let (a, b) = (line.points[1], line.points[2]);
        assert_eq!(line.label_at, ((a.0 + b.0) / 2., (a.1 + b.1) / 2.));
        // The label stays between the two boxes of its gap.
        let half = line.label.chars().count() as f32 * LABEL_CHAR_WIDTH / 2.;
        let (customers, orders) = (&l.boxes[1], &l.boxes[0]);
        assert!(line.label_at.0 - half >= customers.right(), "{line:?}");
        assert!(line.label_at.0 + half <= orders.x, "{line:?}");

        // It follows a moved box.
        let mut moved = l.clone();
        move_box(&mut moved, &m, 0, orders.x + 200., orders.y + 90.);
        let line = &moved.lines[0];
        let (a, b) = (line.points[1], line.points[2]);
        assert_eq!(line.label_at, ((a.0 + b.0) / 2., (a.1 + b.1) / 2.));
        assert_ne!(line.label_at, l.lines[0].label_at);

        // A view dependency has none.
        let v = layout(&with_view());
        let dependency = v.lines.iter().find(|l| l.dependency).expect("a dependency");
        assert_eq!(dependency.label, "");

        // A long label moves the layers apart.
        let mut long = model();
        long.relationships[0].from_columns = vec!["c".repeat(37)];
        let wide = layout(&long);
        let label = wide.lines[0].label.chars().count() as f32 * LABEL_CHAR_WIDTH;
        assert_eq!(wide.lines[0].label.chars().count(), 40);
        assert!(wide.boxes[0].x - wide.boxes[1].right() >= label + 2. * LABEL_INSET);
        assert!(wide.boxes[0].x > l.boxes[0].x);

        // The SVG holds the label, escaped and centred.
        let mut odd = model();
        odd.relationships[0].from_columns = vec!["a<b".into()];
        let svg = to_svg(&layout(&odd));
        let doc = roxmltree::Document::parse(&svg).expect("well-formed");
        let text = doc
            .descendants()
            .find(|n| n.attribute("data-label") == Some("orders-customers"))
            .expect("the label of the line");
        assert_eq!(text.text(), Some("a<b:id"));
        assert_eq!(text.attribute("text-anchor"), Some("middle"));
        assert!(svg.contains("a&lt;b:id"));
    }

    #[test]
    fn svg_and_png() {
        let mut m = with_view();
        m.entities[1].name = "a<b>&\"c\"".into();
        m.relationships[0].to = "a<b>&\"c\"".into();
        let l = layout(&m);
        let svg = to_svg(&l);
        let doc = roxmltree::Document::parse(&svg).expect("well-formed");
        let root = doc.root_element();
        let width: f32 = root.attribute("width").unwrap().parse().unwrap();
        assert!((width - l.width).abs() < 0.1);
        let groups = root.children().filter(|n| n.has_tag_name("g")).count();
        assert_eq!(groups, l.boxes.len());
        let paths = root.children().filter(|n| n.has_tag_name("path")).count();
        assert_eq!(paths, l.lines.len());
        assert!(svg.contains("a&lt;b&gt;&amp;&quot;c&quot;"));
        assert!(svg.contains("big_orders (view)"));
        assert_eq!(
            svg.matches("stroke-dasharray").count(),
            2,
            "view box and dependency"
        );
        let png = to_png(&l, 2.).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert!(width.abs_diff((l.width * 2.).round() as u32) <= 2);
        assert!(height.abs_diff((l.height * 2.).round() as u32) <= 2);
    }

    /// The places where `layout` breaks the rule of SPEC-0111 v8 behaviour 16, checked here on its
    /// own: a segment that is not horizontal or vertical, or that crosses or touches a box other
    /// than its line's ends, or its own boxes anywhere but the first and the last segment.
    fn breaches(layout: &DiagramLayout) -> Vec<String> {
        let mut out = Vec::new();
        for l in &layout.lines {
            let index = |n: &str| layout.boxes.iter().position(|b| b.name == n).unwrap();
            let (start, end) = (index(&l.to), index(&l.from));
            let last = l.points.len() - 2;
            for (k, w) in l.points.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                if a.0 != b.0 && a.1 != b.1 {
                    out.push(format!("{}-{}: slanted segment {a:?} {b:?}", l.from, l.to));
                }
                for (i, r) in layout.boxes.iter().enumerate() {
                    if (i == start && k == 0) || (i == end && k == last) {
                        continue;
                    }
                    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
                    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
                    let hit = x0 <= r.x + r.width && x1 >= r.x && y0 <= r.y + r.height && y1 >= r.y;
                    if hit {
                        out.push(format!(
                            "{}-{}: segment {k} {a:?} {b:?} touches {}",
                            l.from, l.to, r.name
                        ));
                    }
                }
            }
        }
        out
    }

    fn typed(name: &str, ty: &str, keys: &[&str]) -> ErColumn {
        ErColumn {
            name: name.into(),
            data_type: ty.into(),
            nullable: false,
            keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        }
    }

    fn key(from: &str, column: &str, to: &str, to_column: &str) -> Relationship {
        Relationship {
            from: from.into(),
            from_columns: vec![column.into()],
            to: to.into(),
            to_columns: vec![to_column.into()],
            many: true,
            optional: false,
        }
    }

    /// For each line, the box it comes nearest to among those it must keep away from (every box but
    /// its ends, and its own boxes away from its first and last segment), with the distance: the
    /// shortest straight distance between a segment and the box's rectangle, measured here on its
    /// own (SPEC-0111 v8 behaviour 16).
    fn nearest(layout: &DiagramLayout) -> Vec<(String, f32, String)> {
        layout
            .lines
            .iter()
            .map(|l| {
                let index = |n: &str| layout.boxes.iter().position(|b| b.name == n).unwrap();
                let (start, end) = (index(&l.to), index(&l.from));
                let last = l.points.len() - 2;
                let mut best = (f32::INFINITY, String::new());
                for (k, w) in l.points.windows(2).enumerate() {
                    let (x0, x1) = (w[0].0.min(w[1].0), w[0].0.max(w[1].0));
                    let (y0, y1) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
                    for (i, r) in layout.boxes.iter().enumerate() {
                        if (i == start && k == 0) || (i == end && k == last) {
                            continue;
                        }
                        let dx = (r.x - x1).max(x0 - (r.x + r.width)).max(0.);
                        let dy = (r.y - y1).max(y0 - (r.y + r.height)).max(0.);
                        let d = dx.hypot(dy);
                        if d < best.0 {
                            best = (d, r.name.clone());
                        }
                    }
                }
                (format!("{}-{}", l.from, l.to), best.0, best.1)
            })
            .collect()
    }

    /// The PostgreSQL schema of OBS-0068 in the shape the window check of #867 drew it: `item`, the
    /// wide `pd_note` and the narrow `warehouse` left; `movement`, `pd_move`, the view
    /// `pd_note_large` and the narrower `stock` in the middle; the view `stock_value` at the top
    /// right, using `item` (over the middle layer) and `stock`.
    fn obs_0068() -> ErModel {
        let view = |name: &str, layer: usize, row: usize, columns: Vec<ErColumn>| {
            let mut e = entity(name, layer, row, columns);
            e.view = true;
            e
        };
        ErModel {
            entities: vec![
                entity(
                    "item",
                    0,
                    0,
                    vec![
                        typed("sku", "text", &["PK"]),
                        typed("name", "text", &[]),
                        typed("unit_price_cents", "integer", &[]),
                    ],
                ),
                entity(
                    "pd_note",
                    0,
                    1,
                    vec![
                        typed("id", "integer", &["PK"]),
                        typed("body", "text", &["UK"]),
                        typed("amount_cents", "numeric(12,2)", &[]),
                    ],
                ),
                entity(
                    "warehouse",
                    0,
                    2,
                    vec![typed("id", "integer", &["PK"]), typed("city", "text", &[])],
                ),
                entity(
                    "movement",
                    1,
                    0,
                    vec![
                        typed("id", "integer", &["PK"]),
                        typed("warehouse_id", "integer", &["FK"]),
                        typed("sku", "text", &["FK"]),
                        typed("delta", "integer", &[]),
                        typed("moved_at", "timestamp without time zone", &[]),
                    ],
                ),
                entity(
                    "pd_move",
                    1,
                    1,
                    vec![
                        typed("id", "integer", &["PK"]),
                        typed("warehouse_id", "integer", &["FK"]),
                        typed("note", "text", &[]),
                    ],
                ),
                view(
                    "pd_note_large",
                    1,
                    2,
                    vec![typed("id", "integer", &[]), typed("body", "text", &[])],
                ),
                entity(
                    "stock",
                    1,
                    3,
                    vec![
                        typed("warehouse_id", "integer", &["PK", "FK"]),
                        typed("sku", "text", &["PK", "FK"]),
                        typed("qty", "integer", &[]),
                    ],
                ),
                view(
                    "stock_value",
                    2,
                    0,
                    vec![
                        typed("warehouse_id", "integer", &[]),
                        typed("value_cents", "bigint", &[]),
                    ],
                ),
            ],
            relationships: vec![
                key("movement", "warehouse_id", "warehouse", "id"),
                key("movement", "sku", "item", "sku"),
                key("pd_move", "warehouse_id", "warehouse", "id"),
                key("stock", "warehouse_id", "warehouse", "id"),
                key("stock", "sku", "item", "sku"),
            ],
            outside: vec![],
            dependencies: vec![
                Dependency {
                    view: "pd_note_large".into(),
                    uses: "pd_note".into(),
                },
                Dependency {
                    view: "stock_value".into(),
                    uses: "item".into(),
                },
                Dependency {
                    view: "stock_value".into(),
                    uses: "stock".into(),
                },
            ],
        }
    }

    /// The routes of behaviour 3 (before v8) for the lines of `layout`, in the same order.
    fn plain_routes(model: &ErModel, layout: &DiagramLayout) -> Vec<Vec<(f32, f32)>> {
        let find = |n: &str| layout.boxes.iter().find(|b| b.name == n).unwrap();
        model
            .relationships
            .iter()
            .filter_map(|r| line(r, model, &layout.boxes).map(|l| l.points))
            .chain(model.dependencies.iter().map(|d| {
                let (used, view) = (find(&d.uses), find(&d.view));
                connect(used, used.row_y(None), view, view.row_y(None))
            }))
            .collect()
    }

    /// SPEC-0111/AK-15 (v8, OBS-0068 L9, window check of #867 E1).
    #[test]
    fn lines_go_around_boxes() {
        let m = obs_0068();
        let l = layout(&m);
        let by = |n: &str| l.boxes.iter().find(|b| b.name == n).unwrap();
        // The geometry of the window check: pd_note is wider than warehouse, movement wider than
        // stock, and stock_value stands at the top of the third layer.
        assert_eq!((by("pd_note").right(), by("movement").x), (296., 416.));
        assert_eq!((by("movement").right(), by("stock").right()), (760., 644.));
        assert_eq!((by("stock_value").x, by("stock_value").y), (880., MARGIN));

        // The routes of behaviour 3: warehouse → movement 4 px beside pd_note, stock → stock_value
        // 2 px beside movement up to its header row, item → stock_value over movement's header.
        let plain = plain_routes(&m, &l);
        let mut before = l.clone();
        for (line, points) in before.lines.iter_mut().zip(&plain) {
            line.points = points.clone();
        }
        let close = nearest(&before);
        let at = |list: &[(String, f32, String)], name: &str| {
            list.iter().find(|(n, _, _)| n == name).cloned().unwrap()
        };
        assert_eq!(
            at(&close, "movement-warehouse"),
            ("movement-warehouse".into(), 4., "pd_note".into())
        );
        assert_eq!(
            at(&close, "stock_value-stock"),
            ("stock_value-stock".into(), 2., "movement".into())
        );
        let k = l
            .lines
            .iter()
            .position(|x| x.from == "stock_value" && x.to == "stock")
            .unwrap();
        assert!(
            plain[k]
                .iter()
                .any(|p| p.1 == by("movement").y + HEADER_HEIGHT / 2.)
        );
        let old = breaches(&before);
        assert!(
            old.iter()
                .any(|b| b.starts_with("stock_value-item") && b.ends_with("touches movement")),
            "{old:#?}"
        );

        // After: the basic rule, and 8 px from every box a line must keep away from.
        assert_eq!(breaches(&l), Vec::<String>::new());
        for (line, d, near) in nearest(&l) {
            assert!(
                d >= er_route::CLEARANCE - 1e-3,
                "{line} runs {d} px from {near}"
            );
        }
        let rects: Vec<Rect> = l.boxes.iter().map(BoxLayout::rect).collect();
        let index = |n: &str| l.boxes.iter().position(|b| b.name == n).unwrap();
        for (line, points) in l.lines.iter().zip(&plain) {
            assert_eq!(line.points.first(), points.first(), "same start");
            assert_eq!(line.points.last(), points.last(), "same end");
            assert_eq!(line.label_at, middle(&line.points));
            let (s, e) = (index(&line.to), index(&line.from));
            if er_route::keeps_clear(points, &rects, s, e, er_route::CLEARANCE) {
                // A route of behaviour 3 that keeps 8 px stays, up to the lane of behaviour 15.
                assert_eq!(line.points.len(), points.len(), "{line:?}");
                for (a, b) in line.points.iter().zip(points) {
                    assert_eq!(a.1, b.1);
                    assert_eq!((a.0 - b.0) % LANE, 0., "{a:?} {b:?}");
                }
            } else {
                assert_ne!(
                    &line.points, points,
                    "{}-{} is led around",
                    line.from, line.to
                );
            }
        }
        // The two lines of E1 now run through the middle of their gap.
        let route = |from: &str, to: &str| {
            l.lines
                .iter()
                .find(|x| x.from == from && x.to == to)
                .unwrap()
                .points
                .clone()
        };
        let up = route("movement", "warehouse");
        assert!(
            up.iter().all(|p| p.0 <= by("warehouse").right()
                || p.0 >= by("pd_note").right() + er_route::CLEARANCE),
            "{up:?}"
        );
        let view = route("stock_value", "stock");
        assert!(
            view[1..view.len() - 1]
                .iter()
                .all(|p| p.0 >= by("movement").right() + er_route::CLEARANCE),
            "{view:?}"
        );

        // Lanes (behaviour 15, v8 behaviour 16): no two inner segments of one direction on one
        // another.
        for vertical in [true, false] {
            let at = |p: (f32, f32)| if vertical { p.0 } else { p.1 };
            let along = |p: (f32, f32)| if vertical { p.1 } else { p.0 };
            let inner: Vec<(usize, f32, f32, f32)> = l
                .lines
                .iter()
                .enumerate()
                .flat_map(|(i, x)| {
                    let n = x.points.len();
                    x.points
                        .windows(2)
                        .enumerate()
                        .filter(move |(k, w)| {
                            *k > 0 && *k + 1 < n - 1 && at(w[0]) == at(w[1]) && w[0] != w[1]
                        })
                        .map(move |(_, w)| {
                            let (a, b) = (along(w[0]), along(w[1]));
                            (i, at(w[0]), a.min(b), a.max(b))
                        })
                })
                .collect();
            for (n, a) in inner.iter().enumerate() {
                for b in &inner[n + 1..] {
                    assert!(
                        a.0 == b.0 || (a.1 - b.1).abs() >= LANE / 2. || a.3 < b.2 || b.3 < a.2,
                        "{a:?} and {b:?} lie on one another"
                    );
                }
            }
        }

        // The SVG draws the same routes.
        let svg = to_svg(&l);
        let doc = roxmltree::Document::parse(&svg).unwrap();
        let paths: Vec<Vec<(f32, f32)>> = doc
            .descendants()
            .filter(|n| n.has_tag_name("path"))
            .map(|n| {
                n.attribute("d")
                    .unwrap()
                    .split(['M', 'L'])
                    .filter(|p| !p.trim().is_empty())
                    .map(|p| {
                        let mut v = p.split_whitespace().map(|v| v.parse::<f32>().unwrap());
                        (v.next().unwrap(), v.next().unwrap())
                    })
                    .collect()
            })
            .collect();
        assert_eq!(paths.len(), l.lines.len());
        for (path, line) in paths.iter().zip(&l.lines) {
            assert_eq!(path.len(), line.points.len());
            for (a, b) in path.iter().zip(&line.points) {
                assert!((a.0 - b.0).abs() <= 0.05 && (a.1 - b.1).abs() <= 0.05);
            }
        }
    }

    /// SPEC-0111/AK-15 (v8): the rule after random moves of the boxes, and for a self-reference with
    /// a box in its loop.
    #[test]
    fn lines_stay_clear_of_moved_boxes() {
        let m = obs_0068();
        let base = layout(&m);
        let mut seed: u64 = 0x5eed_0068;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut checked = 0;
        for round in 0..200 {
            let mut boxes = base.boxes.clone();
            let mut placed: Vec<Rect> = Vec::new();
            let mut ok = true;
            for b in &mut boxes {
                let mut tries = 0;
                loop {
                    let x = MARGIN + (next() % 1400) as f32;
                    let y = MARGIN + (next() % 900) as f32;
                    let r = Rect {
                        x0: x,
                        y0: y,
                        x1: x + b.width,
                        y1: y + b.height,
                    };
                    let g = r.grown(16.);
                    if placed
                        .iter()
                        .all(|p| g.x1 <= p.x0 || p.x1 <= g.x0 || g.y1 <= p.y0 || p.y1 <= g.y0)
                    {
                        b.x = x;
                        b.y = y;
                        placed.push(r);
                        break;
                    }
                    tries += 1;
                    if tries > 500 {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            let l = route(&m, boxes);
            assert_eq!(breaches(&l), Vec::<String>::new(), "round {round}");
            for (line, d, near) in nearest(&l) {
                assert!(
                    d >= 2. - 1e-3,
                    "round {round}: {line} runs {d} px from {near}"
                );
            }
            checked += 1;
        }
        assert!(checked >= 190, "only {checked} rounds placed");

        // A self-reference whose loop would run through the box beside it.
        let mut own = model();
        own.entities[2] = entity("beside", 2, 0, vec![col("id", &["PK"], false)]);
        let mut l = layout(&own);
        let orders = l.boxes.iter().position(|b| b.name == "orders").unwrap();
        let beside = l.boxes.iter().position(|b| b.name == "beside").unwrap();
        let (right, top) = (l.boxes[orders].right(), l.boxes[orders].y);
        move_box(&mut l, &own, beside, right + 16., top);
        let loop_line = l.lines.iter().find(|x| x.from == x.to).unwrap();
        assert!(
            loop_line.points.iter().all(|p| p.0 < right + 16.),
            "{loop_line:?}"
        );
        assert_eq!(breaches(&l), Vec::<String>::new());
        for (line, d, near) in nearest(&l) {
            assert!(d >= 2. - 1e-3, "{line} runs {d} px from {near}");
        }
    }
}
