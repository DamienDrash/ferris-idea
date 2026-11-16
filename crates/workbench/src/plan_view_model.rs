//! The views of the Plan tab without a screen (`SPEC-0110` v13 behaviours 19 and 20, v14
//! behaviour 21): which view is shown, the boxes and lines of the diagram and the zoom that fits
//! it, the bars of the flame view, the properties of the selected step and the collapsed hints.
//! Steps are numbered in tree order, parents before children, as the rows of the Tree view
//! (`db_compare_model::plan_rows`).

use idecraft_database::{Plan, PlanNode};

/// The views of the Plan tab, in the order ⌥R goes through them (v13 behaviour 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanMode {
    /// One row per step (behaviour 2).
    #[default]
    Tree,
    /// Boxes with lines from each child to its parent.
    Diagram,
    /// Bars as wide as their share of the parent.
    Flame,
    /// The plan as the database reported it (behaviour 10).
    Raw,
}

impl PlanMode {
    /// Every view, in the order of the switches.
    pub const ALL: [PlanMode; 4] = [
        PlanMode::Tree,
        PlanMode::Diagram,
        PlanMode::Flame,
        PlanMode::Raw,
    ];

    /// The switch's label.
    pub fn label(self) -> &'static str {
        match self {
            PlanMode::Tree => "Tree",
            PlanMode::Diagram => "Diagram",
            PlanMode::Flame => "Flame",
            PlanMode::Raw => "Raw",
        }
    }

    /// The view ⌥R goes to: the next one, from Raw back to Tree.
    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }

    /// Whether the view shows steps that can be selected (all but Raw).
    pub fn has_steps(self) -> bool {
        self != PlanMode::Raw
    }
}

/// What the bars of the flame view measure (v13 behaviour 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlameMetric {
    /// The estimated total cost (the default).
    #[default]
    Total,
    /// The estimated start-up cost.
    Startup,
    /// The measured time × loops, for a measured plan.
    Time,
}

impl FlameMetric {
    /// Every metric, in the order of the switches.
    pub const ALL: [FlameMetric; 3] = [FlameMetric::Total, FlameMetric::Startup, FlameMetric::Time];

    /// The switch's label.
    pub fn label(self) -> &'static str {
        match self {
            FlameMetric::Total => "Total cost",
            FlameMetric::Startup => "Startup cost",
            FlameMetric::Time => "Time",
        }
    }

    /// Whether the switch is offered for `plan`: Time only for a measured plan.
    pub fn offered(self, plan: &Plan) -> bool {
        self != FlameMetric::Time || plan.analyzed
    }

    /// The step's own value of this metric, if the database reported it.
    fn value(self, n: &PlanNode) -> Option<f64> {
        match self {
            FlameMetric::Total => n.cost,
            FlameMetric::Startup => n.startup_cost,
            FlameMetric::Time => n.actual_ms.map(|ms| ms * n.loops.unwrap_or(1.0).max(1.0)),
        }
    }
}

/// The steps in tree order with their depth and the index of their parent.
pub fn steps(plan: &Plan) -> Vec<(usize, Option<usize>, &PlanNode)> {
    fn walk<'a>(
        n: &'a PlanNode,
        depth: usize,
        parent: Option<usize>,
        out: &mut Vec<(usize, Option<usize>, &'a PlanNode)>,
    ) {
        let me = out.len();
        out.push((depth, parent, n));
        for c in &n.children {
            walk(c, depth + 1, Some(me), out);
        }
    }
    let mut out = Vec::new();
    for n in &plan.nodes {
        walk(n, 0, None, &mut out);
    }
    out
}

/// A share as shown: `62 %`, `0.4 %` below one per cent.
pub fn share_text(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 1.0 {
        format!("{p:.1} %")
    } else {
        format!("{p:.0} %")
    }
}

/// One box of the diagram (v13 behaviour 19).
#[derive(Debug, Clone, PartialEq)]
pub struct DiagramBox {
    /// The step, in tree order.
    pub step: usize,
    /// Its row: 0 for the top-level steps.
    pub depth: usize,
    /// The middle of the box in slots: each step without children takes one slot, a parent stands
    /// over the middle of its children.
    pub x: f32,
    /// The parent's step, to which the line leads.
    pub parent: Option<usize>,
    /// The operation as the database names it.
    pub operation: String,
    /// The object, or empty.
    pub object: String,
    /// The share of the plan's cost, e.g. `62 %`, or empty.
    pub share: String,
    /// Whether the step has a hint (warning colour and ⚠).
    pub hint: bool,
}

/// The boxes of the diagram, in tree order.
pub fn diagram(plan: &Plan) -> Vec<DiagramBox> {
    let steps = steps(plan);
    let mut x = vec![0f32; steps.len()];
    let mut next_slot = 0f32;
    // Children come after their parent in tree order, so walking backwards places every child
    // before its parent; leaves are numbered in tree order first.
    for (i, (_, _, n)) in steps.iter().enumerate() {
        if n.children.is_empty() {
            x[i] = next_slot;
            next_slot += 1.0;
        }
    }
    for i in (0..steps.len()).rev() {
        let kids: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, (_, p, _))| *p == Some(i))
            .map(|(k, _)| k)
            .collect();
        if let (Some(first), Some(last)) = (kids.first(), kids.last()) {
            x[i] = (x[*first] + x[*last]) / 2.0;
        }
    }
    steps
        .iter()
        .enumerate()
        .map(|(i, (depth, parent, n))| DiagramBox {
            step: i,
            depth: *depth,
            x: x[i],
            parent: *parent,
            operation: n.operation.clone(),
            object: n.object.clone().unwrap_or_default(),
            share: n.cost_share.map(share_text).unwrap_or_default(),
            hint: !n.hints.is_empty(),
        })
        .collect()
}

/// The number of slots the diagram is wide and the number of rows it is high.
pub fn diagram_size(boxes: &[DiagramBox]) -> (f32, usize) {
    let width = boxes.iter().map(|b| b.x + 1.0).fold(0f32, f32::max);
    let rows = boxes.iter().map(|b| b.depth + 1).max().unwrap_or(0);
    (width, rows)
}

/// The geometry of the diagram at 100 % (v13 behaviour 19): every box as wide as its longest line
/// (120–320 px) and three lines high, one slot per step without children.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiagramMetrics {
    /// Width of every box.
    pub box_w: f32,
    /// Height of every box.
    pub box_h: f32,
    /// Between a row of boxes and the row below it, where the lines run.
    pub gap: f32,
    /// Between two boxes side by side.
    pub slot_gap: f32,
    /// Width of the whole diagram.
    pub width: f32,
    /// Height of the whole diagram, from the top of the first row to the bottom of the last.
    pub height: f32,
}

/// The geometry of `boxes` for text of `char_width` per character on lines `line_height` high.
pub fn diagram_metrics(boxes: &[DiagramBox], char_width: f32, line_height: f32) -> DiagramMetrics {
    let box_w = boxes
        .iter()
        .map(|b| {
            [
                b.operation.chars().count() + 2,
                b.object.chars().count(),
                b.share.chars().count(),
            ]
            .into_iter()
            .max()
            .unwrap_or(0) as f32
                * char_width
                + 20.
        })
        .fold(120f32, f32::max)
        .min(320.);
    let (box_h, gap, slot_gap) = (line_height * 3. + 8., 28f32, 24f32);
    let (slots, rows) = diagram_size(boxes);
    DiagramMetrics {
        box_w,
        box_h,
        gap,
        slot_gap,
        width: slots * (box_w + slot_gap),
        height: (rows as f32 * (box_h + gap) - gap).max(0.),
    }
}

/// The zoom the diagram is drawn at (v14 behaviour 21). Fitted, it is the ER diagram's fit
/// (`SPEC-0111` behaviour 8): the largest zoom, at most 100 % and at least 25 %, at which the whole
/// diagram is inside `area` (width, height), and 100 % while the area is not known yet. Not
/// fitted, it is 100 %.
pub fn diagram_zoom(metrics: &DiagramMetrics, area: (f32, f32), fit: bool) -> f32 {
    if fit {
        idecraft_database::er_layout::fit_zoom(metrics.width, metrics.height, area.0, area.1)
    } else {
        1.0
    }
}

/// The hints the head of the Plan tab shows (v14 behaviour 21): every hint while they are open
/// or when there is only one, otherwise the first.
pub fn shown_hints(hints: &[String], open: bool) -> &[String] {
    if open || hints.len() < 2 {
        hints
    } else {
        &hints[..1]
    }
}

/// The switch at the end of the hint line (v14 behaviour 21): with several hints, how many more
/// there are while they are collapsed, the way back while they are open; none for one hint.
pub fn hint_toggle(hints: usize, open: bool) -> Option<String> {
    match (hints, open) {
        (0 | 1, _) => None,
        (2, false) => Some("1 more hint ▾".into()),
        (n, false) => Some(format!("{} more hints ▾", n - 1)),
        (_, true) => Some("fewer hints ▴".into()),
    }
}

/// One bar of the flame view (v13 behaviour 19).
#[derive(Debug, Clone, PartialEq)]
pub struct FlameBar {
    /// The step, in tree order.
    pub step: usize,
    /// Its row: 0 for the top-level steps.
    pub depth: usize,
    /// Where it starts, as a part (0 to 1) of the whole width.
    pub start: f64,
    /// How wide it is, as a part of the whole width.
    pub width: f64,
    /// Operation and object.
    pub label: String,
    /// Whether the step has a hint.
    pub hint: bool,
}

/// The bars of the flame view by `metric`: the top-level steps over the whole width, each child
/// across the part of its parent its value takes. A step without a value takes the sum of its
/// children's (or one part); a plan without any value of the metric splits every parent equally.
pub fn flame(plan: &Plan, metric: FlameMetric) -> Vec<FlameBar> {
    let steps = steps(plan);
    let any = steps
        .iter()
        .any(|(_, _, n)| metric.value(n).is_some_and(|v| v > 0.0));
    fn weight(n: &PlanNode, metric: FlameMetric, any: bool) -> f64 {
        if !any {
            return 1.0;
        }
        match metric.value(n) {
            Some(v) if v > 0.0 => v,
            _ => {
                let sum: f64 = n.children.iter().map(|c| weight(c, metric, any)).sum();
                if sum > 0.0 { sum } else { 1.0 }
            }
        }
    }
    let mut out = Vec::new();
    #[allow(clippy::too_many_arguments)]
    fn place(
        nodes: &[PlanNode],
        start: f64,
        width: f64,
        whole: f64,
        depth: usize,
        metric: FlameMetric,
        any: bool,
        next: &mut usize,
        out: &mut Vec<FlameBar>,
    ) {
        let weights: Vec<f64> = nodes.iter().map(|n| weight(n, metric, any)).collect();
        let sum: f64 = weights.iter().sum();
        let scale = if sum > 0.0 {
            width / sum.max(whole)
        } else {
            0.0
        };
        let mut at = start;
        for (n, w) in nodes.iter().zip(weights) {
            let me = *next;
            *next += 1;
            let span = w * scale;
            out.push(FlameBar {
                step: me,
                depth,
                start: at,
                width: span,
                label: match &n.object {
                    Some(o) => format!("{} {o}", n.operation),
                    None => n.operation.clone(),
                },
                hint: !n.hints.is_empty(),
            });
            // Equal parts split the whole parent; values leave room for the parent's own part.
            let own = if any { w } else { 0.0 };
            place(
                &n.children,
                at,
                span,
                own,
                depth + 1,
                metric,
                any,
                next,
                out,
            );
            at += span;
        }
    }
    let mut next = 0;
    place(
        &plan.nodes,
        0.0,
        1.0,
        0.0,
        0,
        metric,
        any,
        &mut next,
        &mut out,
    );
    out.sort_by_key(|b| b.step);
    debug_assert_eq!(out.len(), steps.len());
    out
}

/// The properties of step `step` as listed below the views (v13 behaviour 20): every field the
/// database reported, then the share of the plan's cost and the hints.
pub fn properties(plan: &Plan, step: usize) -> Vec<(String, String)> {
    let Some((_, _, n)) = steps(plan).into_iter().nth(step) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = n
        .properties
        .iter()
        .map(|p| (p.name.clone(), p.value.clone()))
        .collect();
    if let Some(share) = n.cost_share {
        out.push(("Share of the plan's cost".into(), share_text(share)));
    }
    out.extend(n.hints.iter().map(|h| ("⚠ Hint".to_owned(), h.clone())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hash Join (200) over Seq Scan movement (150) and Hash (20) over Seq Scan item (20),
    /// measured.
    fn plan() -> Plan {
        let json = r#"[{"Plan": {"Node Type": "Hash Join", "Startup Cost": 25.0, "Total Cost": 200.0,
            "Plan Rows": 100, "Actual Total Time": 10.0, "Actual Rows": 100, "Actual Loops": 1, "Plans": [
            {"Node Type": "Seq Scan", "Relation Name": "movement", "Alias": "m", "Startup Cost": 0.0,
             "Total Cost": 150.0, "Plan Rows": 10000, "Actual Total Time": 6.0, "Actual Rows": 10000,
             "Actual Loops": 1},
            {"Node Type": "Hash", "Startup Cost": 20.0, "Total Cost": 20.0, "Plan Rows": 5,
             "Actual Total Time": 1.0, "Actual Rows": 5, "Actual Loops": 1, "Plans": [
              {"Node Type": "Seq Scan", "Relation Name": "item", "Alias": "i", "Startup Cost": 0.0,
               "Total Cost": 20.0, "Plan Rows": 5, "Actual Total Time": 0.5, "Actual Rows": 5,
               "Actual Loops": 2}]}]}}]"#;
        let mut nodes = idecraft_database::parse_postgres(json).unwrap();
        idecraft_database::fill_steps(&mut nodes);
        Plan {
            hints: idecraft_database::hints(&nodes),
            nodes,
            analyzed: true,
            rolled_back: true,
            raw: json.into(),
        }
    }

    /// SPEC-0110 v13 AK-23: boxes in rows, each parent over its children, lines to the parent.
    #[test]
    fn diagram_places_children_below_their_parent() {
        let boxes = diagram(&plan());
        #[allow(clippy::type_complexity)]
        let shown: Vec<(usize, f32, Option<usize>, &str, &str, bool)> = boxes
            .iter()
            .map(|b| {
                (
                    b.depth,
                    b.x,
                    b.parent,
                    b.operation.as_str(),
                    b.share.as_str(),
                    b.hint,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (0, 0.5, None, "Hash Join", "100 %", false),
                (1, 0.0, Some(0), "Seq Scan", "75 %", true),
                (1, 1.0, Some(0), "Hash", "10 %", false),
                (2, 1.0, Some(2), "Seq Scan", "10 %", false),
            ]
        );
        assert_eq!(boxes[1].object, "movement m");
        assert_eq!(diagram_size(&boxes), (2.0, 3));
    }

    /// SPEC-0110 v13 AK-23: bars by total cost, start-up cost and measured time; equal parts
    /// without values.
    #[test]
    fn flame_widths_follow_the_metric() {
        let p = plan();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let total = flame(&p, FlameMetric::Total);
        assert!(near(total[0].width, 1.0));
        assert!(near(total[1].width, 0.75) && near(total[1].start, 0.0));
        assert!(near(total[2].width, 0.1) && near(total[2].start, 0.75));
        assert!(
            near(total[3].width, 0.1),
            "the only child fills its parent's value"
        );
        assert_eq!(total[1].label, "Seq Scan movement m");
        assert!(total[1].hint);

        let startup = flame(&p, FlameMetric::Startup);
        // 25 at the top; movement has none (0) and takes one part, Hash 20.
        assert!(near(startup[0].width, 1.0));
        assert!(near(startup[2].width, 20.0 / 25.0), "{startup:?}");

        let time = flame(&p, FlameMetric::Time);
        assert!(near(time[1].width, 0.6), "6 of 10 ms");
        assert!(near(time[3].width, 0.1), "0.5 ms × 2 loops of 10");
        assert!(FlameMetric::Time.offered(&p));

        let mut unmeasured = p.clone();
        unmeasured.analyzed = false;
        assert!(!FlameMetric::Time.offered(&unmeasured));

        // SQLite: no costs, every parent split equally.
        let r = |v: &[&str]| v.iter().map(|c| Some((*c).to_owned())).collect::<Vec<_>>();
        let mut nodes = idecraft_database::parse_sqlite(&[
            r(&["1", "0", "0", "SCAN o"]),
            r(&[
                "2",
                "0",
                "0",
                "SEARCH c USING INTEGER PRIMARY KEY (rowid=?)",
            ]),
        ]);
        idecraft_database::fill_steps(&mut nodes);
        let lite = Plan {
            hints: Vec::new(),
            nodes,
            analyzed: false,
            rolled_back: false,
            raw: String::new(),
        };
        let bars = flame(&lite, FlameMetric::Total);
        assert!(
            near(bars[0].width, 0.5) && near(bars[1].start, 0.5),
            "{bars:?}"
        );
    }

    /// SPEC-0110 v13 AK-23: ⌥R's order and the views with steps.
    #[test]
    fn views_cycle_and_steps_move_in_every_view() {
        let order: Vec<&str> = std::iter::successors(Some(PlanMode::Tree), |m| Some(m.next()))
            .take(5)
            .map(PlanMode::label)
            .collect();
        assert_eq!(order, ["Tree", "Diagram", "Flame", "Raw", "Tree"]);
        assert!(PlanMode::Diagram.has_steps() && PlanMode::Flame.has_steps());
        assert!(!PlanMode::Raw.has_steps());
        assert_eq!(steps(&plan()).len(), diagram(&plan()).len());
        assert_eq!(
            steps(&plan()).len(),
            flame(&plan(), FlameMetric::Total).len()
        );
    }

    /// SPEC-0110 v14 AK-26: the diagram's size at 100 %, the zoom that fits it into an area as
    /// the ER diagram's fit does, and the hints collapsed to the first with a count.
    #[test]
    fn the_diagram_fits_its_area_and_hints_collapse() {
        let m = diagram_metrics(&diagram(&plan()), 7.8, 24.);
        // "Hash Join" and its ⚠ room are the longest line: 11 × 7.8 + 20 < 120, so 120 px.
        assert_eq!((m.box_w, m.box_h, m.gap, m.slot_gap), (120., 80., 28., 24.));
        assert_eq!((m.width, m.height), (288., 296.), "2 slots, 3 rows");
        assert_eq!(
            diagram_zoom(&m, (288., 296.), true),
            1.0,
            "it fits at 100 %"
        );
        assert_eq!(
            diagram_zoom(&m, (2000., 2000.), true),
            1.0,
            "never above 100 %"
        );
        assert_eq!(diagram_zoom(&m, (144., 600.), true), 0.5, "by width");
        assert_eq!(diagram_zoom(&m, (1000., 148.), true), 0.5, "by height");
        assert_eq!(diagram_zoom(&m, (10., 10.), true), 0.25, "at least 25 %");
        assert_eq!(diagram_zoom(&m, (0., 0.), true), 1.0, "area not known yet");
        assert_eq!(diagram_zoom(&m, (144., 148.), false), 1.0, "100 % chosen");
        assert_eq!(diagram_metrics(&[], 7.8, 24.).height, 0.);

        let hints: Vec<String> = ["full scan of `a`", "full scan of `b`", "misestimate"]
            .map(String::from)
            .to_vec();
        assert_eq!(shown_hints(&hints, false), &hints[..1]);
        assert_eq!(shown_hints(&hints, true), &hints[..]);
        assert_eq!(
            shown_hints(&hints[..1], false),
            &hints[..1],
            "one hint stays"
        );
        assert!(shown_hints(&[], false).is_empty());
        assert_eq!(hint_toggle(3, false).as_deref(), Some("2 more hints ▾"));
        assert_eq!(hint_toggle(2, false).as_deref(), Some("1 more hint ▾"));
        assert_eq!(hint_toggle(3, true).as_deref(), Some("fewer hints ▴"));
        assert_eq!(hint_toggle(1, false), None, "nothing to fold");
        assert_eq!(hint_toggle(0, true), None);
    }

    /// SPEC-0110 v13 AK-24: every field, the share and the hint, in order.
    #[test]
    fn properties_of_a_step() {
        let p = plan();
        let props = properties(&p, 1);
        let names: Vec<&str> = props.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "Node Type",
                "Relation Name",
                "Alias",
                "Startup Cost",
                "Total Cost",
                "Plan Rows",
                "Actual Total Time",
                "Actual Rows",
                "Actual Loops",
                "Share of the plan's cost",
                "⚠ Hint"
            ]
        );
        assert_eq!(props[9].1, "75 %");
        assert!(props[10].1.starts_with("full scan of `movement m`"));
        assert!(properties(&p, 99).is_empty());
        assert_eq!(share_text(0.004), "0.4 %");
    }
}
