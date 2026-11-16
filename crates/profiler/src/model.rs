//! The profile: a call tree with total and self cost (behaviour 3).

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
}

/// Self and total cost per function and location.
/// Per function and file: self cost, total, and the line with the most self cost.
type Spots = BTreeMap<(String, Option<String>), (f64, f64, Option<(String, u32)>, f64)>;

/// Functions by self cost, most expensive first; functions without self cost are left out.
pub fn hot_spots(profile: &Profile) -> Vec<HotSpot> {
    let mut by_name: Spots = BTreeMap::new();
    fn walk(n: &ProfileNode, stack: &mut Vec<String>, out: &mut Spots) {
        // One row per function, not per line (hand test 47: a C function with -g came twice).
        let key = (n.name.clone(), n.location.as_ref().map(|(f, _)| f.clone()));
        let entry = out.entry(key).or_insert((0.0, 0.0, None, -1.0));
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
        .map(|((name, _), (self_cost, total, location, _))| HotSpot {
            name,
            location,
            self_cost,
            total,
        })
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
}
