//! Lanes and lines of the commit graph in the Git log tool window (`SPEC-0086` v3 behaviour 8),
//! free of the UI toolkit.
//!
//! Every row is cut in two halves at the commit's dot: the upper half carries the lines from the
//! row above down to the dot's height, the lower half from there to the row below. Lines change
//! lanes only in the row of a commit, so a line passing a commit stays straight.

/// A line in one half of a row: from lane `from` at the half's top to lane `to` at its bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// Lane at the top of the half.
    pub from: usize,
    /// Lane at the bottom of the half.
    pub to: usize,
    /// Colour index of the line (behaviour 8: each line keeps its colour down to its end).
    pub color: usize,
}

/// The graph of one row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphRow {
    /// Lane of the commit's dot.
    pub lane: usize,
    /// Colour index of the dot.
    pub color: usize,
    /// Lines from the top of the row to the dot's height.
    pub up: Vec<Edge>,
    /// Lines from the dot's height to the bottom of the row.
    pub down: Vec<Edge>,
}

impl GraphRow {
    /// Lanes the row needs: one more than the rightmost lane it uses.
    pub fn width(&self) -> usize {
        self.up
            .iter()
            .chain(&self.down)
            .flat_map(|e| [e.from, e.to])
            .chain([self.lane])
            .max()
            .map_or(1, |m| m + 1)
    }
}

/// A line waiting for a commit further down.
#[derive(Debug, Clone)]
struct Open {
    /// Full id of the commit the line leads to.
    id: String,
    color: usize,
}

/// Lays out the graph of `commits`, given as (id, parent ids) newest first in list order.
pub fn layout<'a, I, P>(commits: I) -> Vec<GraphRow>
where
    I: IntoIterator<Item = (&'a str, P)>,
    P: IntoIterator<Item = &'a str>,
{
    let mut lanes: Vec<Option<Open>> = Vec::new();
    let mut next_color = 0;
    let mut rows = Vec::new();
    for (id, parents) in commits {
        let reaching: Vec<usize> = (0..lanes.len())
            .filter(|&j| lanes[j].as_ref().is_some_and(|o| o.id == id))
            .collect();
        let (lane, color) = match reaching.first() {
            Some(&j) => (j, lanes[j].as_ref().map_or(0, |o| o.color)),
            None => (free_lane(&lanes), fresh(&mut next_color)),
        };
        let mut row = GraphRow {
            lane,
            color,
            ..GraphRow::default()
        };
        for (j, open) in lanes.iter().enumerate() {
            let Some(open) = open else {
                continue;
            };
            let to = if open.id == id { lane } else { j };
            row.up.push(Edge {
                from: j,
                to,
                color: open.color,
            });
        }
        for &j in &reaching {
            lanes[j] = None;
        }
        // Lines passing the commit go straight on.
        for (j, open) in lanes.iter().enumerate() {
            if let Some(open) = open {
                row.down.push(Edge {
                    from: j,
                    to: j,
                    color: open.color,
                });
            }
        }
        for (k, parent) in parents.into_iter().enumerate() {
            let waiting = (0..lanes.len())
                .find(|&j| j != lane && lanes[j].as_ref().is_some_and(|o| o.id == parent));
            let (to, color) = if k == 0 {
                (lane, color)
            } else if let Some(j) = waiting {
                (j, lanes[j].as_ref().map_or(0, |o| o.color))
            } else {
                let j = free_lane_except(&lanes, lane);
                (j, fresh(&mut next_color))
            };
            if lanes.len() <= to {
                lanes.resize(to + 1, None);
            }
            if lanes[to].is_none() {
                lanes[to] = Some(Open {
                    id: parent.to_owned(),
                    color,
                });
            }
            row.down.push(Edge {
                from: lane,
                to,
                color,
            });
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(row);
    }
    rows
}

/// A colour index no line had before.
fn fresh(next: &mut usize) -> usize {
    let c = *next;
    *next += 1;
    c
}

/// The leftmost lane without a line.
fn free_lane(lanes: &[Option<Open>]) -> usize {
    lanes
        .iter()
        .position(Option::is_none)
        .unwrap_or(lanes.len())
}

/// The leftmost lane without a line other than `taken`.
fn free_lane_except(lanes: &[Option<Open>], taken: usize) -> usize {
    (0..)
        .find(|&j| j != taken && lanes.get(j).is_none_or(Option::is_none))
        .unwrap_or(lanes.len())
}

/// Rows with only the dots, each in the first lane, for a filtered list (`SPEC-0086` v3
/// behaviour 3): lines between the remaining commits would join commits that are not related.
pub fn dots_only(count: usize) -> Vec<GraphRow> {
    vec![GraphRow::default(); count]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(commits: &[(&'static str, &[&'static str])]) -> Vec<GraphRow> {
        layout(commits.iter().map(|(id, ps)| (*id, ps.iter().copied())))
    }

    fn e(from: usize, to: usize, color: usize) -> Edge {
        Edge { from, to, color }
    }

    #[test]
    fn straight_history_stays_in_one_lane() {
        let rows = run(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
        assert!(rows.iter().all(|r| r.lane == 0 && r.color == 0));
        assert_eq!(rows[0].up, []);
        assert_eq!(rows[0].down, [e(0, 0, 0)]);
        assert_eq!(rows[1].up, [e(0, 0, 0)]);
        assert_eq!(rows[2].down, []);
        assert!(rows.iter().all(|r| r.width() == 1));
    }

    #[test]
    fn a_branch_forks_in_its_own_lane_and_joins_its_fork_point() {
        // feature tip `f` above main's `m`, both on `base`.
        let rows = run(&[("f", &["base"]), ("m", &["base"]), ("base", &[])]);
        assert_eq!((rows[0].lane, rows[0].color), (0, 0));
        assert_eq!(
            (rows[1].lane, rows[1].color),
            (1, 1),
            "the second tip gets a lane"
        );
        assert_eq!(rows[1].up, [e(0, 0, 0)], "f's line passes m");
        assert_eq!(rows[1].down, [e(0, 0, 0), e(1, 1, 1)]);
        assert_eq!(rows[2].lane, 0, "the fork point takes the leftmost line");
        assert_eq!(
            rows[2].up,
            [e(0, 0, 0), e(1, 0, 1)],
            "both lines meet at base"
        );
        assert_eq!(rows[2].down, []);
    }

    #[test]
    fn a_merge_draws_a_line_to_its_second_parent() {
        let rows = run(&[
            ("merge", &["m1", "s1"]),
            ("s1", &["base"]),
            ("m1", &["base"]),
            ("base", &[]),
        ]);
        assert_eq!(rows[0].down, [e(0, 0, 0), e(0, 1, 1)]);
        assert_eq!(
            (rows[1].lane, rows[1].color),
            (1, 1),
            "the side commit in its lane"
        );
        assert_eq!(rows[1].up, [e(0, 0, 0), e(1, 1, 1)]);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[3].up, [e(0, 0, 0), e(1, 0, 1)]);
        assert_eq!(rows[0].width(), 2);
    }

    #[test]
    fn a_merge_joins_the_lane_already_waiting_for_its_parent() {
        // `tip` already waits for `s` when the merge names it as second parent.
        let rows = run(&[
            ("tip", &["s"]),
            ("merge", &["m", "s"]),
            ("s", &[]),
            ("m", &[]),
        ]);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].down, [e(0, 0, 0), e(1, 1, 1), e(1, 0, 0)]);
    }

    #[test]
    fn a_freed_lane_is_used_by_the_next_tip() {
        let rows = run(&[
            ("a2", &["a1"]),
            ("b1", &[]),
            ("a1", &["root"]),
            ("c1", &["root"]),
            ("root", &[]),
        ]);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(
            rows[1].down,
            [e(0, 0, 0)],
            "b's line ends at its root commit"
        );
        assert_eq!(rows[3].lane, 1, "c reuses the lane b left");
        assert_ne!(rows[3].color, rows[1].color, "a new line, a new colour");
    }

    #[test]
    fn lines_passing_a_commit_stay_straight_and_unknown_parents_run_on() {
        let rows = run(&[("x", &["outside"]), ("y", &["z"]), ("z", &[])]);
        for r in &rows[1..] {
            assert!(r.up.contains(&e(0, 0, 0)));
        }
        assert!(
            rows[2].down.contains(&e(0, 0, 0)),
            "the line to a parent beyond the list runs on"
        );
        assert_eq!(dots_only(2), vec![GraphRow::default(); 2]);
    }
}
