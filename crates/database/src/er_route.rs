//! Lines around boxes in the ER diagram (`SPEC-0111` v8 behaviour 16): the rule a route must keep
//! — no segment comes closer than [`CLEARANCE`] to a box that is not one of its ends, nor to its own
//! boxes away from its end points (2 px where boxes stand too close for that), and never touches a
//! foreign box — and an orthogonal route that keeps it when the plain route of behaviour 3 does not.
//!
//! The route is a shortest path with bend costs on a grid of lanes: vertical lanes through the
//! middle of every free strip between the (slightly enlarged) box edges, horizontal lanes likewise
//! plus the rows of the two end points. Between two neighbouring lanes lies at most one box edge,
//! so a step between them is clear exactly when both lane crossings are outside every enlarged box.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// A box's rectangle in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x0: f32,
    /// Top edge.
    pub y0: f32,
    /// Right edge.
    pub x1: f32,
    /// Bottom edge.
    pub y1: f32,
}

impl Rect {
    /// The rectangle grown by `d` on every side.
    pub fn grown(self, d: f32) -> Rect {
        Rect {
            x0: self.x0 - d,
            y0: self.y0 - d,
            x1: self.x1 + d,
            y1: self.y1 + d,
        }
    }

    fn overlaps(&self, o: &Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
}

/// Where a line leaves or enters a box: the point on the box's side and the side, `1.` for the
/// right side (the line runs to the right) and `-1.` for the left side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Port {
    /// The point on the box's edge.
    pub at: (f32, f32),
    /// `1.` away to the right, `-1.` away to the left.
    pub out: f32,
}

/// The least distance a route around boxes keeps from every box (behaviour 16).
pub const CLEARANCE: f32 = 8.;
/// The distance kept where [`CLEARANCE`] leaves no way (boxes closer than 16 px).
const TIGHT: f32 = 2.;
/// What a bend costs, in pixels of length.
const BEND: f64 = 40.;
/// How much a step costs per pixel it lies away from the middle between the ends, relative to its
/// length: small enough to only choose between routes of equal length and bends.
const TIE: f64 = 1e-5;
/// Margins of the search areas around the two end points before the whole canvas is searched.
const AREAS: [f32; 2] = [64., 256.];
/// Room around all boxes for the outermost lanes.
const RING: f32 = 32.;

/// Whether segment `a`–`b` crosses or touches the closed rectangle `r` (any direction).
pub fn crosses(a: (f32, f32), b: (f32, f32), r: &Rect) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0f32, 1f32);
    for (p, q) in [
        (-dx, a.0 - r.x0),
        (dx, r.x1 - a.0),
        (-dy, a.1 - r.y0),
        (dy, r.y1 - a.1),
    ] {
        if p == 0. {
            if q < 0. {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0. {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Whether the axis-parallel segment `a`–`b` comes into the open inside of `r` (touching its edge
/// does not count).
pub fn enters(a: (f32, f32), b: (f32, f32), r: &Rect) -> bool {
    a.0.max(b.0) > r.x0 && a.0.min(b.0) < r.x1 && a.1.max(b.1) > r.y0 && a.1.min(b.1) < r.y1
}

/// Whether segment `a`–`b` comes closer than `room` to the rectangle `r` (into `r` widened by `room`
/// on every side); with `room` 0, whether it touches `r` at all.
pub fn too_close(a: (f32, f32), b: (f32, f32), r: &Rect, room: f32) -> bool {
    if room > 0. {
        enters(a, b, &r.grown(room))
    } else {
        crosses(a, b, r)
    }
}

/// Whether the polyline `points` from box `start` to box `end` (indices into `boxes`, equal for a
/// self-reference) keeps `room` from every box (behaviour 16; with `room` 0: touches none): only the
/// first segment may come closer to `start` and only the last to `end`, where they leave and enter
/// them.
pub fn keeps_clear(
    points: &[(f32, f32)],
    boxes: &[Rect],
    start: usize,
    end: usize,
    room: f32,
) -> bool {
    keeps_clear_between(points, 0..points.len(), boxes, start, end, room)
}

/// [`keeps_clear`] for the segments `segments` of `points` only (segment `k` joins points `k` and
/// `k + 1`).
pub fn keeps_clear_between(
    points: &[(f32, f32)],
    segments: std::ops::Range<usize>,
    boxes: &[Rect],
    start: usize,
    end: usize,
    room: f32,
) -> bool {
    let last = points.len().saturating_sub(2);
    points
        .windows(2)
        .enumerate()
        .filter(|(k, _)| segments.contains(k))
        .all(|(k, w)| {
            boxes.iter().enumerate().all(|(i, r)| {
                let allowed = (i == start && k == 0) || (i == end && k == last);
                allowed || !too_close(w[0], w[1], r, room)
            })
        })
}

/// An orthogonal route from `start` (on box `own[0]`) to `end` (on box `own[1]`) that keeps
/// [`CLEARANCE`] from every box of `boxes`, its own included, except where it leaves and enters its
/// own boxes; with the fewest bends and the shortest way, and of equal routes the one nearest the
/// middle between the end points. Tries a tighter distance where boxes stand close. `None` when there
/// is no way out (an end point covered or closed in by another box).
pub fn around(start: Port, end: Port, boxes: &[Rect], own: [usize; 2]) -> Option<Vec<(f32, f32)>> {
    let span = Rect {
        x0: start.at.0.min(end.at.0),
        y0: start.at.1.min(end.at.1),
        x1: start.at.0.max(end.at.0),
        y1: start.at.1.max(end.at.1),
    };
    [CLEARANCE, TIGHT].into_iter().find_map(|room| {
        AREAS
            .iter()
            .map(|m| Some(span.grown(*m)))
            .chain(std::iter::once(None))
            .find_map(|area| search(start, end, boxes, own, room, area))
    })
}

/// The lanes between sorted `edges`: the middle of each strip between two neighbouring edges.
fn lanes(mut edges: Vec<f32>) -> Vec<f32> {
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    edges.windows(2).map(|w| (w[0] + w[1]) / 2.).collect()
}

#[derive(Clone, Copy, PartialEq)]
struct State {
    cost: f64,
    node: usize,
    vertical: bool,
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.node.cmp(&self.node))
            .then_with(|| other.vertical.cmp(&self.vertical))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One search on the lanes inside `area` (the whole drawing when `None`), boxes grown by `room`.
fn search(
    start: Port,
    end: Port,
    boxes: &[Rect],
    own: [usize; 2],
    room: f32,
    area: Option<Rect>,
) -> Option<Vec<(f32, f32)>> {
    let grown: Vec<Rect> = boxes.iter().map(|b| b.grown(room)).collect();
    let area = area.unwrap_or_else(|| {
        grown
            .iter()
            .fold(
                Rect {
                    x0: start.at.0.min(end.at.0),
                    y0: start.at.1.min(end.at.1),
                    x1: start.at.0.max(end.at.0),
                    y1: start.at.1.max(end.at.1),
                },
                |a, b| Rect {
                    x0: a.x0.min(b.x0),
                    y0: a.y0.min(b.y0),
                    x1: a.x1.max(b.x1),
                    y1: a.y1.max(b.y1),
                },
            )
            .grown(RING)
    });
    // The canvas starts at 0, 0: no lane left of or above it.
    let area = Rect {
        x0: area.x0.max(0.),
        y0: area.y0.max(0.),
        ..area
    };
    let inside: Vec<usize> = (0..grown.len())
        .filter(|&i| grown[i].overlaps(&area))
        .collect();
    let clip = |v: f32, lo: f32, hi: f32| v.clamp(lo, hi);
    let xs = lanes(
        inside
            .iter()
            .flat_map(|&i| [grown[i].x0, grown[i].x1])
            .map(|v| clip(v, area.x0, area.x1))
            .chain([area.x0, area.x1])
            .collect(),
    );
    let mut ys = lanes(
        inside
            .iter()
            .flat_map(|&i| [grown[i].y0, grown[i].y1])
            .map(|v| clip(v, area.y0, area.y1))
            .chain([area.y0, area.y1])
            .collect(),
    );
    // The rows of the end points exactly, so the first and last segments stay horizontal.
    ys.retain(|y| (y - start.at.1).abs() >= 0.01 && (y - end.at.1).abs() >= 0.01);
    ys.extend([start.at.1, end.at.1]);
    ys.sort_by(f32::total_cmp);
    ys.dedup();
    let (nx, ny) = (xs.len(), ys.len());
    if nx == 0 || ny == 0 {
        return None;
    }
    // A lane crossing is blocked when it lies strictly inside a grown box (own boxes included); the
    // bits say which: 1 another box, 2 the start's box, 4 the end's box.
    let mut mark = vec![0u8; nx * ny];
    for &i in &inside {
        let r = &grown[i];
        let ix0 = xs.partition_point(|&x| x <= r.x0);
        let ix1 = xs.partition_point(|&x| x < r.x1);
        let iy0 = ys.partition_point(|&y| y <= r.y0);
        let iy1 = ys.partition_point(|&y| y < r.y1);
        let bits = if own.contains(&i) {
            (if i == own[0] { 2 } else { 0 }) | (if i == own[1] { 4 } else { 0 })
        } else {
            1
        };
        for j in iy0..iy1 {
            for k in ix0..ix1 {
                mark[j * nx + k] |= bits;
            }
        }
    }
    let row = |y: f32| ys.iter().position(|&v| v == y);
    // The first free crossing on the end point's row, walking away from its box (`mine`, its bit);
    // only that box may lie on the way.
    let exit = |p: Port, mine: usize, bit: u8| -> Option<usize> {
        let j = row(p.at.1)?;
        let mut order: Vec<usize> = (0..nx).filter(|&k| (xs[k] - p.at.0) * p.out > 0.).collect();
        if p.out < 0. {
            order.reverse();
        }
        for k in order {
            let n = j * nx + k;
            if mark[n] & !bit != 0 {
                return None;
            }
            if mark[n] == 0 {
                let hop_clear = boxes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == mine || !too_close(p.at, (xs[k], p.at.1), b, room));
                return hop_clear.then_some(n);
            }
        }
        None
    };
    // A self-reference leaves and enters the same box.
    let (sb, eb) = if own[0] == own[1] { (6, 6) } else { (2, 4) };
    let (from, to) = (exit(start, own[0], sb)?, exit(end, own[1], eb)?);
    let point = |n: usize| (xs[n % nx], ys[n / nx]);
    let (xm, ym) = (
        f64::from((start.at.0 + end.at.0) / 2.),
        f64::from((start.at.1 + end.at.1) / 2.),
    );
    let hop = |p: Port, n: usize| f64::from((point(n).0 - p.at.0).abs());
    let mut best = vec![f64::INFINITY; nx * ny * 2];
    let mut back = vec![usize::MAX; nx * ny * 2];
    let mut heap = BinaryHeap::new();
    let key = |n: usize, v: bool| n * 2 + usize::from(v);
    best[key(from, false)] = hop(start, from);
    heap.push(State {
        cost: hop(start, from),
        node: from,
        vertical: false,
    });
    let mut done: Option<(f64, usize)> = None;
    while let Some(State {
        cost,
        node,
        vertical,
    }) = heap.pop()
    {
        if cost > best[key(node, vertical)] {
            continue;
        }
        if done.is_some_and(|(c, _)| cost >= c) {
            break;
        }
        if node == to {
            let total = cost + if vertical { BEND } else { 0. } + hop(end, to);
            if done.is_none_or(|(c, _)| total < c) {
                done = Some((total, key(node, vertical)));
            }
        }
        let (k, j) = (node % nx, node / nx);
        let steps = [
            (k > 0).then(|| (node - 1, false)),
            (k + 1 < nx).then(|| (node + 1, false)),
            (j > 0).then(|| (node - nx, true)),
            (j + 1 < ny).then(|| (node + nx, true)),
        ];
        for (next, v) in steps.into_iter().flatten() {
            if mark[next] != 0 {
                continue;
            }
            let (a, b) = (point(node), point(next));
            let length = f64::from((a.0 - b.0).abs() + (a.1 - b.1).abs());
            let off = if v {
                (f64::from(a.0) - xm).abs()
            } else {
                (f64::from(a.1) - ym).abs()
            };
            let c = cost + length * (1. + TIE * off) + if v != vertical { BEND } else { 0. };
            if c < best[key(next, v)] {
                best[key(next, v)] = c;
                back[key(next, v)] = key(node, vertical);
                heap.push(State {
                    cost: c,
                    node: next,
                    vertical: v,
                });
            }
        }
    }
    let (_, mut at) = done?;
    let mut path = vec![end.at];
    loop {
        path.push(point(at / 2));
        let prev = back[at];
        if prev == usize::MAX {
            break;
        }
        at = prev;
    }
    path.push(start.at);
    path.reverse();
    Some(simplified(path))
}

/// `points` without repeated points and without points in the middle of a straight run.
pub fn simplified(points: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            if (a.0 == b.0 && b.0 == p.0) || (a.1 == b.1 && b.1 == p.1) {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    #[test]
    fn segments_cross_touch_or_miss_a_rectangle() {
        let r = rect(10., 10., 20., 20.);
        assert!(crosses((0., 15.), (30., 15.), &r), "through");
        assert!(crosses((0., 10.), (30., 10.), &r), "along the edge");
        assert!(
            crosses((20., 0.), (20., 5.), &rect(0., 5., 20., 6.)),
            "end on the corner"
        );
        assert!(!crosses((0., 9.9), (30., 9.9), &r), "just above");
        assert!(!crosses((0., 0.), (9., 30.), &r), "beside, slanted");
        assert!(crosses((0., 0.), (30., 30.), &r), "slanted through");
        assert!(crosses((12., 12.), (12., 12.), &r), "a point inside");
        // Distance: 8 px from the box is far enough, 7.9 px is not; touching counts at room 0.
        assert!(!too_close((0., 2.), (30., 2.), &r, 8.));
        assert!(too_close((0., 2.1), (30., 2.1), &r, 8.));
        assert!(
            too_close((22., 0.), (22., 30.), &r, 8.),
            "2 px beside the right edge"
        );
        assert!(!too_close((28., 0.), (28., 30.), &r, 8.));
        assert!(
            too_close((20., 0.), (20., 10.), &r, 0.),
            "touching the corner"
        );
        assert!(!too_close((20., 0.), (20., 9.), &r, 0.));
    }

    #[test]
    fn a_route_goes_around_a_box_in_its_way() {
        // Start box, a box in the middle, end box, all on one row.
        let boxes = [
            rect(0., 0., 100., 100.),
            rect(200., 0., 300., 100.),
            rect(400., 0., 500., 100.),
        ];
        let start = Port {
            at: (100., 50.),
            out: 1.,
        };
        let end = Port {
            at: (400., 50.),
            out: -1.,
        };
        let route = around(start, end, &boxes, [0, 2]).expect("a route");
        assert_eq!(route.first(), Some(&start.at));
        assert_eq!(route.last(), Some(&end.at));
        assert!(keeps_clear(&route, &boxes, 0, 2, 0.), "{route:?}");
        assert!(
            route
                .windows(2)
                .all(|w| w[0].0 == w[1].0 || w[0].1 == w[1].1),
            "orthogonal: {route:?}"
        );
        // Around the middle box: two bends to its gap, two to the far gap.
        assert_eq!(route.len(), 6, "{route:?}");
        // Covered end point: no route.
        let covered = [boxes[0], rect(90., 40., 150., 60.), boxes[2]];
        assert_eq!(around(start, end, &covered, [0, 2]), None);
    }

    #[test]
    fn simplified_drops_repeats_and_straight_middles() {
        assert_eq!(
            simplified(vec![(0., 0.), (5., 0.), (5., 0.), (10., 0.), (10., 5.)]),
            vec![(0., 0.), (10., 0.), (10., 5.)]
        );
    }
}
