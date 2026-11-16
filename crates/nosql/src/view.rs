//! Views of a document result (`SPEC-0311` behaviour 6): tree, grid and JSON.

use mongodb::bson::{Bson, Document};

/// BSON type name shown next to a value.
pub fn bson_type(value: &Bson) -> &'static str {
    match value {
        Bson::String(_) => "string",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Double(_) => "double",
        Bson::Decimal128(_) => "decimal128",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::ObjectId(_) => "objectId",
        Bson::Null | Bson::Undefined => "null",
        Bson::Array(_) => "array",
        Bson::Document(_) => "object",
        Bson::Binary(_) => "binary",
        Bson::RegularExpression(_) => "regex",
        Bson::Timestamp(_) => "timestamp",
        _ => "other",
    }
}

/// Compact relaxed Extended JSON of a value.
pub fn compact(value: &Bson) -> String {
    value.clone().into_relaxed_extjson().to_string()
}

/// Text of a grid cell: strings as they are, everything else compact JSON (dates as ISO text,
/// object ids as their hex).
pub fn cell_text(value: &Bson) -> String {
    match value {
        Bson::String(s) => s.clone(),
        Bson::ObjectId(o) => o.to_hex(),
        Bson::DateTime(d) => d
            .try_to_rfc3339_string()
            .unwrap_or_else(|_| d.timestamp_millis().to_string()),
        Bson::Int32(n) => n.to_string(),
        Bson::Int64(n) => n.to_string(),
        Bson::Double(f) => f.to_string(),
        Bson::Boolean(b) => b.to_string(),
        Bson::Null => "null".into(),
        Bson::Decimal128(d) => d.to_string(),
        other => compact(other),
    }
}

/// One row of the tree view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// 0 for a document, 1 for its fields, …
    pub depth: usize,
    /// Field name, array index `[n]`, or the document's `_id`.
    pub label: String,
    /// Short value text; for objects and arrays their size (`{3 fields}`, `[2 items]`).
    pub value: String,
    /// BSON type.
    pub kind: &'static str,
}

/// The tree of `docs`, fully expanded (the window collapses by depth).
pub fn tree_rows(docs: &[Document]) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let label = doc
            .get("_id")
            .map(cell_text)
            .unwrap_or_else(|| format!("({})", i + 1));
        rows.push(TreeRow {
            depth: 0,
            label,
            value: format!("{{{} fields}}", doc.len()),
            kind: "object",
        });
        push_fields(doc, 1, &mut rows);
    }
    rows
}

fn push_fields(doc: &Document, depth: usize, rows: &mut Vec<TreeRow>) {
    for (k, v) in doc {
        push_value(k.clone(), v, depth, rows);
    }
}

fn push_value(label: String, v: &Bson, depth: usize, rows: &mut Vec<TreeRow>) {
    let value = match v {
        Bson::Document(d) => format!("{{{} fields}}", d.len()),
        Bson::Array(a) => format!("[{} items]", a.len()),
        other => cell_text(other),
    };
    rows.push(TreeRow {
        depth,
        label,
        value,
        kind: bson_type(v),
    });
    match v {
        Bson::Document(d) => push_fields(d, depth + 1, rows),
        Bson::Array(a) => {
            for (i, item) in a.iter().enumerate() {
                push_value(format!("[{i}]"), item, depth + 1, rows);
            }
        }
        _ => {}
    }
}

/// The grid view: columns are the top-level fields in first-seen order with `_id` first; missing
/// fields are `None`.
pub fn grid(docs: &[Document]) -> (Vec<String>, Vec<Vec<Option<String>>>) {
    let mut columns: Vec<String> = Vec::new();
    if docs.iter().any(|d| d.contains_key("_id")) {
        columns.push("_id".into());
    }
    for d in docs {
        for k in d.keys() {
            if !columns.iter().any(|c| c == k) {
                columns.push(k.clone());
            }
        }
    }
    let rows = docs
        .iter()
        .map(|d| columns.iter().map(|c| d.get(c).map(cell_text)).collect())
        .collect();
    (columns, rows)
}

/// The JSON view: relaxed Extended JSON, pretty printed, as an array.
pub fn to_json(docs: &[Document]) -> String {
    let values: Vec<serde_json::Value> = docs
        .iter()
        .map(|d| Bson::Document(d.clone()).into_relaxed_extjson())
        .collect();
    serde_json::to_string_pretty(&values).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId};

    fn sample() -> Vec<Document> {
        let id = ObjectId::parse_str("65a1b2c3d4e5f60718293a4b").unwrap();
        vec![
            doc! {"name": "pen", "_id": id, "tags": ["a", "b"], "dim": {"w": 1.5}},
            doc! {"_id": 2, "name": "ink", "price": 3_i64},
        ]
    }

    #[test]
    fn tree_with_types() {
        let rows = tree_rows(&sample());
        let show: Vec<String> = rows
            .iter()
            .map(|r| {
                format!(
                    "{}{} = {} ({})",
                    "  ".repeat(r.depth),
                    r.label,
                    r.value,
                    r.kind
                )
            })
            .collect();
        assert_eq!(
            show,
            [
                "65a1b2c3d4e5f60718293a4b = {4 fields} (object)",
                "  name = pen (string)",
                "  _id = 65a1b2c3d4e5f60718293a4b (objectId)",
                "  tags = [2 items] (array)",
                "    [0] = a (string)",
                "    [1] = b (string)",
                "  dim = {1 fields} (object)",
                "    w = 1.5 (double)",
                "2 = {3 fields} (object)",
                "  _id = 2 (int32)",
                "  name = ink (string)",
                "  price = 3 (int64)",
            ]
        );
    }

    #[test]
    fn grid_union_with_id_first() {
        let (cols, rows) = grid(&sample());
        assert_eq!(cols, ["_id", "name", "tags", "dim", "price"]);
        assert_eq!(rows[0][2].as_deref(), Some(r#"["a","b"]"#));
        assert_eq!(rows[0][3].as_deref(), Some(r#"{"w":1.5}"#));
        assert_eq!(rows[0][4], None);
        assert_eq!(rows[1][4].as_deref(), Some("3"));
    }

    #[test]
    fn json_is_relaxed_extended_json() {
        let json = to_json(&sample());
        assert!(
            json.contains(r#""$oid": "65a1b2c3d4e5f60718293a4b""#),
            "{json}"
        );
        assert!(json.contains(r#""price": 3"#), "{json}");
        assert!(json.starts_with('['));
        let d = doc! {"at": mongodb::bson::DateTime::from_millis(0)};
        assert_eq!(cell_text(d.get("at").unwrap()), "1970-01-01T00:00:00Z");
        assert!(to_json(&[d]).contains(r#""$date": "1970-01-01T00:00:00Z""#));
    }
}
