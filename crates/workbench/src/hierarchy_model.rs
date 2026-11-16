//! Rows of the call hierarchy tool window (`SPEC-0082` v2): the tree of callers or callees as the
//! visible rows with expand and collapse, free of the UI toolkit.

use std::collections::HashSet;

use idecraft_languages::insight::CallNode;

/// Position of a node in the tree: child indices from the root (the root is `[]`).
pub type NodePath = Vec<usize>;

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Where the node is in the tree.
    pub path: NodePath,
    /// Nesting depth (0 = the function the hierarchy is for).
    pub depth: usize,
    /// Whether the node has children to show.
    pub has_children: bool,
    /// Whether its children are shown.
    pub expanded: bool,
}

/// The node at `path`.
pub fn node<'a>(root: &'a CallNode, path: &[usize]) -> Option<&'a CallNode> {
    path.iter().try_fold(root, |n, &i| n.children.get(i))
}

/// The visible rows: the root and, below every expanded node, its children (behaviour 6).
pub fn rows(root: &CallNode, expanded: &HashSet<NodePath>) -> Vec<Row> {
    fn walk(n: &CallNode, path: NodePath, expanded: &HashSet<NodePath>, out: &mut Vec<Row>) {
        let open = !n.children.is_empty() && expanded.contains(&path);
        out.push(Row {
            depth: path.len(),
            has_children: !n.children.is_empty(),
            expanded: open,
            path: path.clone(),
        });
        if open {
            for (i, c) in n.children.iter().enumerate() {
                let mut p = path.clone();
                p.push(i);
                walk(c, p, expanded, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Vec::new(), expanded, &mut out);
    out
}

/// Only the root expanded, so its direct callers (callees) show (Collapse All, `SPEC-0082` v4
/// behaviour 10).
pub fn root_expanded() -> HashSet<NodePath> {
    HashSet::from([Vec::new()])
}

/// The paths expanded when the tool window opens: every node with children (`SPEC-0082` v4
/// behaviour 8).
pub fn initially_expanded(root: &CallNode) -> HashSet<NodePath> {
    fn walk(n: &CallNode, path: NodePath, out: &mut HashSet<NodePath>) {
        if n.children.is_empty() {
            return;
        }
        for (i, c) in n.children.iter().enumerate() {
            let mut p = path.clone();
            p.push(i);
            walk(c, p, out);
        }
        out.insert(path);
    }
    let mut out = HashSet::new();
    walk(root, Vec::new(), &mut out);
    out
}

/// `root` with the children of every node sorted by name, ignoring case (behaviour 10).
pub fn sorted(root: &CallNode) -> CallNode {
    let mut n = root.clone();
    n.children = root.children.iter().map(sorted).collect();
    n.children
        .sort_by_cached_key(|c| row_label(&c.name).to_lowercase());
    n
}

/// A function's name without the return type the server appends (`priceOf(Item, Discount) :
/// int` → `priceOf(Item, Discount)`, behaviour 9).
pub fn row_label(name: &str) -> &str {
    match name.rfind(") : ") {
        Some(i) => &name[..=i],
        None => name,
    }
}

/// The grey text of a row: the server's detail in parentheses, else `file:line`, and the number
/// of calls when there are several (behaviour 9).
pub fn row_detail(n: &CallNode) -> String {
    let place = match n.detail.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => format!("({d})"),
        _ => format!(
            "{}:{}",
            n.location
                .path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default(),
            n.location.range.start_line + 1
        ),
    };
    match n.call_sites.len() {
        0 | 1 => place,
        k => format!("{place}  ({k} calls)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_languages::insight::{Location, Range};

    fn n(name: &str, children: Vec<CallNode>) -> CallNode {
        CallNode {
            name: name.into(),
            kind: "function".into(),
            detail: None,
            location: Location {
                path: "a.c".into(),
                range: Range {
                    start_line: 0,
                    start_character: 0,
                    end_line: 0,
                    end_character: 1,
                },
            },
            call_sites: Vec::new(),
            children,
            recursive: false,
            mark: None,
        }
    }

    #[test]
    fn rows_follow_expansion() {
        let tree = n(
            "leaf",
            vec![n("mid", vec![n("top", vec![])]), n("rec", vec![])],
        );
        let mut open = root_expanded();
        let names = |rows: &[Row]| -> Vec<String> {
            rows.iter()
                .map(|r| node(&tree, &r.path).unwrap().name.clone())
                .collect()
        };
        let r = rows(&tree, &open);
        assert_eq!(names(&r), ["leaf", "mid", "rec"]);
        assert!(r[0].expanded && r[1].has_children && !r[1].expanded && !r[2].has_children);
        open.insert(vec![0]);
        let r = rows(&tree, &open);
        assert_eq!(names(&r), ["leaf", "mid", "top", "rec"]);
        assert_eq!(r[2].depth, 2);
        open.remove(&Vec::new());
        assert_eq!(names(&rows(&tree, &open)), ["leaf"]);
        assert!(node(&tree, &[5]).is_none());
    }

    /// SPEC-0082 v4 AK-6.
    #[test]
    fn every_level_starts_open() {
        let tree = n(
            "leaf",
            vec![
                n("mid", vec![n("top", vec![n("main", vec![])])]),
                n("rec", vec![]),
            ],
        );
        let open = initially_expanded(&tree);
        let r = rows(&tree, &open);
        assert_eq!(r.len(), 5);
        assert_eq!(r[3].depth, 3);
        assert_eq!(rows(&tree, &root_expanded()).len(), 3);
    }

    /// SPEC-0082 v4 AK-6.
    #[test]
    fn sorted_by_name() {
        let tree = n(
            "apply",
            vec![
                n("zeta", vec![n("b", vec![]), n("A", vec![])]),
                n("alpha", vec![]),
            ],
        );
        let s = sorted(&tree);
        assert_eq!(s.children[0].name, "alpha");
        assert_eq!(s.children[1].children[0].name, "A");
        assert_eq!(
            tree.children[0].name, "zeta",
            "the original keeps its order"
        );
    }

    /// SPEC-0082 v4 AK-6.
    #[test]
    fn row_label_drops_the_return_type() {
        assert_eq!(
            row_label("priceOf(Item, Discount) : int"),
            "priceOf(Item, Discount)"
        );
        assert_eq!(row_label("calc"), "calc");
        let mut x = n("priceOf(Item, Discount) : int", vec![]);
        x.detail = Some("lab.inventory.Discounts".into());
        assert_eq!(row_detail(&x), "(lab.inventory.Discounts)");
        x.detail = None;
        x.call_sites = vec![x.location.range.clone(), x.location.range.clone()];
        assert_eq!(row_detail(&x), "a.c:1  (2 calls)");
    }
}
