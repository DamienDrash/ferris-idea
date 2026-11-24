//! The profile: a call tree with total and self cost (behaviour 3), costs per line (`SPEC-0120`
//! v5 behaviour 10) and the tree folded to the project's frames (v5 behaviour 12).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What the costs count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Samples (one per millisecond of run time).
    Samples,
    /// Seconds of measured time.
    Seconds,
    /// Bytes of memory (`SPEC-0250`).
    Bytes,
}

/// One function in one calling context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileNode {
    /// Function name.
    pub name: String,
    /// Library or module it belongs to, if known.
    pub module: Option<String>,
    /// Source file as reported (may be a bare file name) and 1-based line, if known.
    pub location: Option<(String, u32)>,
    /// Cost including callees.
    pub total: f64,
    /// Cost in the function itself.
    pub self_cost: f64,
    /// Calls, when the profiler counts them (cProfile).
    pub calls: Option<u64>,
    /// Callees, most expensive first.
    pub children: Vec<ProfileNode>,
    /// Whether the frame's source file is in the project (`SPEC-0120` v5 behaviour 11), set when
    /// the profile is stored or read.
    #[serde(default)]
    pub in_project: bool,
}

impl ProfileNode {
    /// A node without children.
    pub fn leaf(name: &str, total: f64) -> Self {
        Self {
            name: name.to_owned(),
            module: None,
            location: None,
            total,
            self_cost: total,
            calls: None,
            children: Vec::new(),
            in_project: false,
        }
    }

    /// Whether this is a thread or another root that is no frame of code (no file, no module):
    /// it is not set apart as code outside the project.
    pub fn is_thread(&self) -> bool {
        self.location.is_none() && self.module.is_none()
    }

    /// The node without its callees.
    fn alone(&self) -> ProfileNode {
        ProfileNode {
            name: self.name.clone(),
            module: self.module.clone(),
            location: self.location.clone(),
            total: self.total,
            self_cost: self.self_cost,
            calls: self.calls,
            children: Vec::new(),
            in_project: self.in_project,
        }
    }

    /// Sorts children by cost and sets each self cost to total minus the children's totals.
    pub(crate) fn finish(&mut self) {
        for c in &mut self.children {
            c.finish();
        }
        self.children.sort_by(|a, b| {
            b.total
                .partial_cmp(&a.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let below: f64 = self.children.iter().map(|c| c.total).sum();
        if self.calls.is_none() {
            self.self_cost = (self.total - below).max(0.0);
        }
    }
}

/// A finished profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// `sample` or `cProfile`.
    pub engine: String,
    /// What the costs count.
    pub unit: Unit,
    /// Cost of the whole run (100 %).
    pub total: f64,
    /// Top-level nodes (threads for `sample`, entry points for `cProfile`).
    pub roots: Vec<ProfileNode>,
    /// Remarks, e.g. why little was measured.
    pub notes: Vec<String>,
    /// Costs per source line recorded from the stacks (`SPEC-0120` v5 behaviour 10); empty when
    /// the tree itself has them (`sample`, cProfile) or the engine has no lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<LineCost>,
}

/// The cost of one source line (`SPEC-0120` v5 behaviour 10): what the samples count whose stack
/// has a frame at the line (once per sample), and what the innermost frame spends there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineCost {
    /// Source file as reported.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// The function the line belongs to.
    pub function: String,
    /// Cost of the stacks through this line.
    pub total: f64,
    /// Cost with this line innermost.
    pub self_cost: f64,
}

/// Collects line costs from whole stacks.
#[derive(Default)]
pub(crate) struct LineCosts {
    by_line: BTreeMap<(String, u32), (String, f64, f64)>,
}

impl LineCosts {
    /// Adds `weight` to every line of `frames` (outermost first) once, and as self cost to the
    /// innermost frame's line when it has one.
    pub(crate) fn add<'a>(
        &mut self,
        frames: impl IntoIterator<Item = (&'a str, Option<&'a (String, u32)>)>,
        weight: f64,
    ) {
        let mut seen: Vec<(&str, u32)> = Vec::new();
        let mut innermost = None;
        for (name, location) in frames {
            innermost = location.map(|l| (name, l));
            let Some((file, line)) = location else {
                continue;
            };
            if seen.contains(&(file.as_str(), *line)) {
                continue;
            }
            seen.push((file, *line));
            self.by_line
                .entry((file.clone(), *line))
                .or_insert_with(|| (name.to_owned(), 0.0, 0.0))
                .1 += weight;
        }
        if let Some((name, (file, line))) = innermost {
            self.by_line
                .entry((file.clone(), *line))
                .or_insert_with(|| (name.to_owned(), 0.0, 0.0))
                .2 += weight;
        }
    }

    /// The costs by file and line.
    pub(crate) fn finish(self) -> Vec<LineCost> {
        self.by_line
            .into_iter()
            .map(|((file, line), (function, total, self_cost))| LineCost {
                file,
                line,
                function,
                total,
                self_cost,
            })
            .collect()
    }
}

/// Line costs read off a tree whose nodes are one line each (`sample`, cProfile's `def` lines): a
/// node's total counts unless a caller already stands at the same line (recursion).
fn lines_of_tree(roots: &[ProfileNode]) -> Vec<LineCost> {
    fn walk<'a>(
        n: &'a ProfileNode,
        above: &mut Vec<(&'a str, u32)>,
        out: &mut BTreeMap<(String, u32), (String, f64, f64)>,
    ) {
        let here = n.location.as_ref().map(|(f, l)| (f.as_str(), *l));
        if let Some((file, line)) = here {
            let e = out
                .entry((file.to_owned(), line))
                .or_insert_with(|| (n.name.clone(), 0.0, 0.0));
            if !above.contains(&(file, line)) {
                e.1 += n.total;
            }
            e.2 += n.self_cost;
            above.push((file, line));
        }
        for c in &n.children {
            walk(c, above, out);
        }
        if here.is_some() {
            above.pop();
        }
    }
    let mut out = BTreeMap::new();
    for r in roots {
        walk(r, &mut Vec::new(), &mut out);
    }
    LineCosts { by_line: out }.finish()
}

impl Profile {
    /// The call tree without nodes below `min_share` of the run (0.01 = 1 %) or deeper than
    /// `max_depth`, for answers that must stay small.
    pub fn pruned(&self, min_share: f64, max_depth: usize) -> Vec<ProfileNode> {
        fn cut(n: &ProfileNode, limit: f64, depth: usize, max: usize) -> ProfileNode {
            let mut out = n.clone();
            out.children = if depth + 1 >= max {
                Vec::new()
            } else {
                n.children
                    .iter()
                    .filter(|c| c.total >= limit)
                    .map(|c| cut(c, limit, depth + 1, max))
                    .collect()
            };
            out
        }
        let limit = self.total * min_share;
        self.roots
            .iter()
            .filter(|r| r.total >= limit)
            .map(|r| cut(r, limit, 0, max_depth))
            .collect()
    }

    /// Costs per source line, by file and line (`SPEC-0120` v5 behaviour 10): those recorded from
    /// the stacks, else read off the tree.
    pub fn line_costs(&self) -> Vec<LineCost> {
        if self.lines.is_empty() {
            lines_of_tree(&self.roots)
        } else {
            self.lines.clone()
        }
    }

    /// Marks every frame whose reported file `in_project` accepts (`SPEC-0120` v5 behaviour 11).
    pub fn mark_project(&mut self, in_project: &mut dyn FnMut(&str) -> bool) {
        fn walk(n: &mut ProfileNode, f: &mut dyn FnMut(&str) -> bool) {
            n.in_project = n.location.as_ref().is_some_and(|(file, _)| f(file));
            for c in &mut n.children {
                walk(c, f);
            }
        }
        for r in &mut self.roots {
            walk(r, in_project);
        }
    }

    /// The profile folded to the project's frames (`SPEC-0120` v5 behaviour 12): a frame outside
    /// the project is left out, its callees in the project move up to the nearest kept caller
    /// (merged with the same function there) and its own cost becomes that caller's self cost.
    /// Roots stay.
    pub fn project_only(&self) -> Profile {
        /// The kept frames below `n` and the self cost of the frames left out on the way.
        fn below(n: &ProfileNode) -> (Vec<ProfileNode>, f64) {
            let mut kept = Vec::new();
            let mut absorbed = 0.0;
            for c in &n.children {
                if c.in_project {
                    kept.push(keep(c));
                } else {
                    absorbed += c.self_cost;
                    let (k, a) = below(c);
                    kept.extend(k);
                    absorbed += a;
                }
            }
            (kept, absorbed)
        }
        fn keep(n: &ProfileNode) -> ProfileNode {
            let (children, absorbed) = below(n);
            let mut out = n.alone();
            out.self_cost += absorbed;
            out.children = merged(children);
            out
        }
        fn merged(nodes: Vec<ProfileNode>) -> Vec<ProfileNode> {
            let mut out: Vec<ProfileNode> = Vec::new();
            for n in nodes {
                let file = |x: &ProfileNode| x.location.as_ref().map(|(f, _)| f.clone());
                match out
                    .iter_mut()
                    .find(|m| m.name == n.name && m.module == n.module && file(m) == file(&n))
                {
                    Some(m) => {
                        if n.total > m.total {
                            m.location = n.location.clone();
                        }
                        m.total += n.total;
                        m.self_cost += n.self_cost;
                        m.calls = match (m.calls, n.calls) {
                            (Some(a), Some(b)) => Some(a + b),
                            (a, b) => a.or(b),
                        };
                        m.children.extend(n.children);
                    }
                    None => out.push(n),
                }
            }
            for m in &mut out {
                let children = std::mem::take(&mut m.children);
                m.children = merged(children);
            }
            out.sort_by(|a, b| {
                b.total
                    .partial_cmp(&a.total)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            out
        }
        Profile {
            roots: self.roots.iter().map(keep).collect(),
            ..self.clone()
        }
    }
}

/// One frame of a recorded stack: function, module and source location, if known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// Function name.
    pub name: String,
    /// Library or module.
    pub module: Option<String>,
    /// Source file and 1-based line.
    pub location: Option<(String, u32)>,
}

impl StackFrame {
    /// A frame with only a name.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            module: None,
            location: None,
        }
    }
}

/// Function in a calling context: name, module and source file (lines may differ between
/// stacks; the costliest line is kept).
type FrameKey = (String, Option<String>, Option<String>);

/// Collects weighted stacks (outermost frame first) into one call tree per root (thread).
#[derive(Default)]
pub struct StackTree {
    roots: BTreeMap<String, TreeNode>,
    lines: LineCosts,
}

#[derive(Default)]
struct TreeNode {
    weight: f64,
    lines: BTreeMap<u32, f64>,
    children: BTreeMap<FrameKey, TreeNode>,
}

impl TreeNode {
    fn node(self, (name, module, file): FrameKey) -> ProfileNode {
        let mut node = ProfileNode::leaf(&name, self.weight);
        node.module = module;
        let line = self
            .lines
            .iter()
            .max_by(|a, b| {
                a.1.partial_cmp(b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.0.cmp(a.0))
            })
            .map(|(line, _)| *line);
        node.location = file.zip(line);
        node.children = self.children.into_iter().map(|(k, t)| t.node(k)).collect();
        node
    }
}

impl StackTree {
    /// Adds `weight` along `frames` (outermost first) below root `root`.
    pub fn add(&mut self, root: &str, frames: &[StackFrame], weight: f64) {
        self.lines.add(
            frames
                .iter()
                .map(|f| (f.name.as_str(), f.location.as_ref())),
            weight,
        );
        let mut node = self.roots.entry(root.to_owned()).or_default();
        node.weight += weight;
        for f in frames {
            let file = f.location.as_ref().map(|(file, _)| file.clone());
            node = node
                .children
                .entry((f.name.clone(), f.module.clone(), file))
                .or_default();
            node.weight += weight;
            if let Some((_, line)) = &f.location {
                *node.lines.entry(*line).or_default() += weight;
            }
        }
    }

    /// The finished profile; roots are sorted by cost.
    pub fn profile(self, engine: &str, unit: Unit) -> Profile {
        let mut roots: Vec<ProfileNode> = self
            .roots
            .into_iter()
            .map(|(name, t)| {
                let mut n = t.node((name, None, None));
                n.finish();
                n
            })
            .collect();
        roots.sort_by(|a, b| {
            b.total
                .partial_cmp(&a.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = roots.iter().map(|r| r.total).sum();
        Profile {
            engine: engine.to_owned(),
            unit,
            total,
            roots,
            notes: Vec::new(),
            lines: self.lines.finish(),
        }
    }
}

/// One function with its cost over all calling contexts (behaviour 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotSpot {
    /// Function name.
    pub name: String,
    /// Source location, if known.
    pub location: Option<(String, u32)>,
    /// Cost in the function itself, summed over all contexts.
    pub self_cost: f64,
    /// Cost including callees, counted once per stack (recursion is not counted twice).
    pub total: f64,
    /// Whether the function's file is in the project (`SPEC-0120` v5 behaviour 11).
    #[serde(default)]
    pub in_project: bool,
}

/// Per function and file: self cost, total, the line with the most self cost and its cost, and
/// whether the file is in the project.
type Spots = BTreeMap<(String, Option<String>), (f64, f64, Option<(String, u32)>, f64, bool)>;

/// Functions by self cost, most expensive first; functions without self cost are left out.
pub fn hot_spots(profile: &Profile) -> Vec<HotSpot> {
    let mut by_name: Spots = BTreeMap::new();
    fn walk(n: &ProfileNode, stack: &mut Vec<String>, out: &mut Spots) {
        // One row per function, not per line (hand test 47: a C function with -g came twice).
        let key = (n.name.clone(), n.location.as_ref().map(|(f, _)| f.clone()));
        let entry = out
            .entry(key)
            .or_insert((0.0, 0.0, None, -1.0, n.in_project));
        entry.0 += n.self_cost;
        if n.self_cost > entry.3 {
            entry.2 = n.location.clone();
            entry.3 = n.self_cost;
        }
        if !stack.contains(&n.name) {
            entry.1 += n.total;
        }
        stack.push(n.name.clone());
        for c in &n.children {
            walk(c, stack, out);
        }
        stack.pop();
    }
    for r in &profile.roots {
        walk(r, &mut Vec::new(), &mut by_name);
    }
    let mut out: Vec<HotSpot> = by_name
        .into_iter()
        .filter(|(_, (s, ..))| *s > 0.0)
        .map(
            |((name, _), (self_cost, total, location, _, in_project))| HotSpot {
                name,
                location,
                self_cost,
                total,
                in_project,
            },
        )
        .collect();
    out.sort_by(|a, b| {
        b.self_cost
            .partial_cmp(&a.self_cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_cost_and_hot_spots_count_recursion_once() {
        // main → f → f (recursive) → g
        let mut g = ProfileNode::leaf("g", 3.0);
        g.location = Some(("x.c".into(), 9));
        let mut inner_f = ProfileNode::leaf("f", 5.0);
        inner_f.children.push(g);
        let mut f = ProfileNode::leaf("f", 8.0);
        f.children.push(inner_f);
        let mut main = ProfileNode::leaf("main", 10.0);
        main.children.push(f);
        main.children.push(ProfileNode::leaf("h", 1.0));
        main.finish();
        assert_eq!(main.self_cost, 1.0);
        assert_eq!(main.children[0].name, "f");
        assert_eq!(main.children[0].self_cost, 3.0);
        let profile = Profile {
            engine: "test".into(),
            unit: Unit::Samples,
            total: 10.0,
            roots: vec![main],
            notes: vec![],
            lines: vec![],
        };
        let hot = hot_spots(&profile);
        let f = hot.iter().find(|h| h.name == "f").unwrap();
        assert_eq!((f.self_cost, f.total), (5.0, 8.0));
        assert_eq!(hot[0].name, "f");
        assert_eq!(hot[1].name, "g");
        assert_eq!(hot[1].location, Some(("x.c".into(), 9)));
        // Two lines of one function make one hot spot at the costlier line.
        let mut a = ProfileNode::leaf("teuer", 6.0);
        a.location = Some(("r.c".into(), 6));
        let mut b = ProfileNode::leaf("teuer", 2.0);
        b.location = Some(("r.c".into(), 7));
        let mut top = ProfileNode::leaf("main", 8.0);
        top.children = vec![a, b];
        top.finish();
        let lines = hot_spots(&Profile {
            roots: vec![top],
            total: 8.0,
            ..profile.clone()
        });
        let teuer: Vec<_> = lines.iter().filter(|h| h.name == "teuer").collect();
        assert_eq!(teuer.len(), 1);
        assert_eq!(teuer[0].self_cost, 8.0);
        assert_eq!(teuer[0].location, Some(("r.c".into(), 6)));
        let small = profile.pruned(0.2, 2);
        assert_eq!(small[0].children.len(), 1, "h (10 %) is cut");
        assert!(small[0].children[0].children.is_empty(), "depth 2");
    }

    #[test]
    fn stacks_merge_by_function_and_keep_the_costliest_line() {
        let at = |name: &str, line: u32| StackFrame {
            name: name.into(),
            module: None,
            location: Some(("a.py".into(), line)),
        };
        let mut t = StackTree::default();
        t.add("main", &[at("main", 1), at("make", 4)], 10.0);
        t.add("main", &[at("main", 2), at("make", 5)], 30.0);
        t.add("main", &[at("main", 2)], 5.0);
        let p = t.profile("test", Unit::Bytes);
        assert_eq!(p.total, 45.0);
        let main = &p.roots[0].children[0];
        assert_eq!((main.total, main.self_cost), (45.0, 5.0));
        assert_eq!(main.location, Some(("a.py".into(), 2)));
        assert_eq!(main.children[0].location, Some(("a.py".into(), 5)));
        assert_eq!(main.children[0].self_cost, 40.0);
    }

    fn at(name: &str, file: &str, line: u32) -> StackFrame {
        StackFrame {
            name: name.into(),
            module: None,
            location: Some((file.into(), line)),
        }
    }

    // SPEC-0120 v5 behaviour 10: every line once per stack, self at the innermost frame.
    #[test]
    fn line_costs_count_a_line_once_per_stack() {
        let mut t = StackTree::default();
        let walk = at("walk", "t.py", 9);
        t.add(
            "main",
            &[
                at("main", "t.py", 20),
                walk.clone(),
                walk.clone(),
                at("leaf", "t.py", 3),
            ],
            4.0,
        );
        t.add(
            "main",
            &[at("main", "t.py", 21), StackFrame::named("native")],
            1.0,
        );
        let p = t.profile("test", Unit::Samples);
        let lines = p.line_costs();
        let get = |l: u32| lines.iter().find(|c| c.line == l).unwrap();
        assert_eq!(
            (get(9).total, get(9).self_cost),
            (4.0, 0.0),
            "recursion once"
        );
        assert_eq!(get(9).function, "walk");
        assert_eq!((get(3).total, get(3).self_cost), (4.0, 4.0));
        assert_eq!((get(20).total, get(20).self_cost), (4.0, 0.0));
        // The innermost frame without a line gives no line its self cost.
        assert_eq!((get(21).total, get(21).self_cost), (1.0, 0.0));
        assert_eq!(lines.len(), 4);
    }

    // A tree of one line per node (`sample`, cProfile's def lines) gives the costs itself.
    #[test]
    fn line_costs_from_a_tree() {
        let mut inner = ProfileNode::leaf("inner", 6.0);
        inner.location = Some(("busy.c".into(), 2));
        let mut again = ProfileNode::leaf("outer", 2.0);
        again.location = Some(("busy.c".into(), 3));
        let mut outer = ProfileNode::leaf("outer", 9.0);
        outer.location = Some(("busy.c".into(), 3));
        outer.children = vec![inner, again];
        let mut thread = ProfileNode::leaf("Thread 1", 10.0);
        thread.children = vec![outer];
        thread.finish();
        let p = Profile {
            engine: "sample".into(),
            unit: Unit::Samples,
            total: 10.0,
            roots: vec![thread],
            notes: vec![],
            lines: vec![],
        };
        let lines = p.line_costs();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].line, lines[0].total, lines[0].self_cost),
            (2, 6.0, 6.0)
        );
        // Line 3 under itself counts its total once; self costs add up.
        assert_eq!(
            (lines[1].line, lines[1].total, lines[1].self_cost),
            (3, 9.0, 3.0)
        );
        assert!(p.roots[0].is_thread() && !p.roots[0].children[0].is_thread());
    }

    // SPEC-0120 v5 behaviour 12: frames outside are left out, their callees move up and merge,
    // their own cost becomes the caller's self cost; totals stay.
    #[test]
    fn the_folded_tree_keeps_totals_and_moves_library_cost() {
        let node = |name: &str, total: f64, own: bool, children: Vec<ProfileNode>| {
            let mut n = ProfileNode::leaf(name, total);
            n.location = Some((format!("{name}.java"), 1));
            n.in_project = own;
            n.children = children;
            n
        };
        // main → forEach (JDK) → lambda (2) ; main → lambda (3) ; main → equals (JDK, 1)
        let lambda_a = node("lambda", 2.0, true, vec![]);
        let for_each = node("forEach", 3.0, false, vec![lambda_a]);
        let lambda_b = node("lambda", 3.0, true, vec![]);
        let equals = node("equals", 1.0, false, vec![]);
        let mut main = node("main", 8.0, true, vec![for_each, lambda_b, equals]);
        let mut thread = ProfileNode::leaf("Thread main", 8.0);
        thread.children = vec![std::mem::replace(&mut main, ProfileNode::leaf("x", 0.0))];
        thread.finish();
        let p = Profile {
            engine: "JFR".into(),
            unit: Unit::Samples,
            total: 8.0,
            roots: vec![thread],
            notes: vec![],
            lines: vec![],
        };
        let folded = p.project_only();
        let main = &folded.roots[0].children[0];
        assert_eq!(main.name, "main");
        assert_eq!(main.total, 8.0);
        // Own 1 + forEach's own 1 + equals 1.
        assert_eq!(main.self_cost, 3.0);
        assert_eq!(main.children.len(), 1);
        assert_eq!(
            (main.children[0].name.as_str(), main.children[0].total),
            ("lambda", 5.0)
        );
        let mut marked = p.clone();
        marked.mark_project(&mut |f| f != "forEach.java" && f != "equals.java");
        let hot = hot_spots(&marked);
        assert!(hot.iter().find(|h| h.name == "lambda").unwrap().in_project);
        assert!(!hot.iter().find(|h| h.name == "forEach").unwrap().in_project);
    }
}
