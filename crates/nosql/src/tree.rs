//! Browser tree models (`SPEC-0310` behaviour 5): field paths sampled from documents, key groups
//! by `:`.

use std::collections::BTreeMap;

use mongodb::bson::{Bson, Document};

use crate::view::bson_type;

/// One field path seen in a sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// Dotted path (`dim.w`); array elements are looked into without an index.
    pub path: String,
    /// BSON types seen, in first-seen order.
    pub types: Vec<&'static str>,
    /// How many sampled documents have the path.
    pub count: usize,
}

/// Field paths of `docs` in first-seen order (behaviour 5).
pub fn field_paths(docs: &[Document]) -> Vec<FieldInfo> {
    let mut out: Vec<FieldInfo> = Vec::new();
    for doc in docs {
        let mut seen: Vec<String> = Vec::new();
        walk(doc, "", &mut out, &mut seen);
    }
    out
}

fn walk(doc: &Document, prefix: &str, out: &mut Vec<FieldInfo>, seen: &mut Vec<String>) {
    for (k, v) in doc {
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        record(&path, v, out, seen);
    }
}

fn record(path: &str, v: &Bson, out: &mut Vec<FieldInfo>, seen: &mut Vec<String>) {
    let kind = bson_type(v);
    let first_in_doc = !seen.iter().any(|s| s == path);
    if first_in_doc {
        seen.push(path.to_owned());
    }
    match out.iter_mut().find(|f| f.path == path) {
        Some(f) => {
            if first_in_doc {
                f.count += 1;
            }
            if !f.types.contains(&kind) {
                f.types.push(kind);
            }
        }
        None => out.push(FieldInfo {
            path: path.to_owned(),
            types: vec![kind],
            count: 1,
        }),
    }
    match v {
        Bson::Document(d) => walk(d, path, out, seen),
        Bson::Array(items) => {
            for item in items {
                if let Bson::Document(d) = item {
                    walk(d, path, out, seen);
                }
            }
        }
        _ => {}
    }
}

/// A node of the key tree: a group (`user:`) or a key, or both.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyNode {
    /// Segment shown (`user`, `42`).
    pub label: String,
    /// Full key when a key ends here.
    pub key: Option<String>,
    /// Children, sorted by label.
    pub children: Vec<KeyNode>,
    /// Number of keys at and under this node.
    pub keys: usize,
}

/// Groups key names by `separator` (behaviour 5). Groups come before keys, both sorted.
pub fn key_tree(keys: &[String], separator: char) -> Vec<KeyNode> {
    #[derive(Default)]
    struct Building {
        key: Option<String>,
        children: BTreeMap<String, Building>,
    }
    let mut root = Building::default();
    for key in keys {
        let mut node = &mut root;
        let parts: Vec<&str> = key.split(separator).collect();
        for part in &parts {
            node = node.children.entry((*part).to_owned()).or_default();
        }
        node.key = Some(key.clone());
    }
    fn finish(label: String, b: Building) -> KeyNode {
        let mut children: Vec<KeyNode> =
            b.children.into_iter().map(|(l, c)| finish(l, c)).collect();
        children.sort_by(|a, b| {
            (a.children.is_empty(), &a.label).cmp(&(b.children.is_empty(), &b.label))
        });
        let keys = usize::from(b.key.is_some()) + children.iter().map(|c| c.keys).sum::<usize>();
        KeyNode {
            label,
            key: b.key,
            children,
            keys,
        }
    }
    finish(String::new(), root).children
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    #[test]
    fn sampled_field_paths_with_types_and_counts() {
        let docs = vec![
            doc! {"_id": 1, "name": "a", "dim": {"w": 1}, "items": [{"sku": "x"}, {"sku": "y"}]},
            doc! {"_id": 2, "name": 5, "dim": {"w": 1.5, "h": 2}},
            doc! {"_id": 3},
        ];
        let f = field_paths(&docs);
        let show: Vec<String> = f
            .iter()
            .map(|f| format!("{} {} {}", f.path, f.types.join("|"), f.count))
            .collect();
        assert_eq!(
            show,
            [
                "_id int32 3",
                "name string|int32 2",
                "dim object 2",
                "dim.w int32|double 2",
                "items array 1",
                "items.sku string 1",
                "dim.h int32 1",
            ]
        );
    }

    #[test]
    fn keys_grouped_by_colon() {
        let keys: Vec<String> = ["user:2", "user:1", "user:1:cart", "session", "user:1:seen"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let tree = key_tree(&keys, ':');
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].label, "user");
        assert_eq!(tree[0].keys, 4);
        assert_eq!(tree[1].key.as_deref(), Some("session"));
        let one = &tree[0].children[0];
        assert_eq!(
            (one.label.as_str(), one.key.as_deref()),
            ("1", Some("user:1"))
        );
        assert_eq!(
            one.children
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            ["cart", "seen"]
        );
        assert_eq!(tree[0].children[1].key.as_deref(), Some("user:2"));
    }
}
