//! Export of documents and key values (`SPEC-0310` behaviour 7, `SPEC-0311` b10, `SPEC-0312` b7).

use std::path::Path;

use mongodb::bson::{Bson, Document};

use crate::StoreError;
use crate::redis_cli::bytes_json;
use crate::redis_session::{KeyValue, ValueData};
use crate::view::{compact, grid};

/// Export file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JSON array, pretty printed.
    Json,
    /// One JSON value per line.
    Jsonl,
    /// Comma-separated values with a header.
    Csv,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// Documents as text (relaxed Extended JSON; CSV: top-level fields, nested values as JSON).
pub fn export_documents(docs: &[Document], format: ExportFormat) -> String {
    match format {
        ExportFormat::Json => crate::view::to_json(docs) + "\n",
        ExportFormat::Jsonl => docs
            .iter()
            .map(|d| compact(&Bson::Document(d.clone())) + "\n")
            .collect(),
        ExportFormat::Csv => {
            let (columns, rows) = grid(docs);
            let mut out = columns
                .iter()
                .map(|c| csv_field(c))
                .collect::<Vec<_>>()
                .join(",");
            out.push('\n');
            for row in rows {
                out.push_str(
                    &row.iter()
                        .map(|c| csv_field(c.as_deref().unwrap_or("")))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                out.push('\n');
            }
            out
        }
    }
}

/// The JSON of one key: key, type, ttl, value.
pub fn key_json(k: &KeyValue) -> serde_json::Value {
    let value = match &k.data {
        ValueData::String(b) => bytes_json(b),
        ValueData::Pairs(p) => serde_json::Value::Array(
            p.iter()
                .map(|(a, b)| serde_json::json!([bytes_json(a), bytes_json(b)]))
                .collect(),
        ),
        ValueData::Items(items) => {
            serde_json::Value::Array(items.iter().map(|b| bytes_json(b)).collect())
        }
        ValueData::Scored(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|(m, s)| serde_json::json!([bytes_json(m), s]))
                .collect(),
        ),
        ValueData::Stream(entries) => serde_json::Value::Array(
            entries
                .iter()
                .map(|(id, fields)| {
                    serde_json::json!({
                        "id": id,
                        "fields": fields.iter().map(|(a, b)| serde_json::json!([bytes_json(a), bytes_json(b)])).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        ),
        ValueData::None => serde_json::Value::Null,
    };
    serde_json::json!({
        "key": bytes_json(&k.key),
        "type": k.kind,
        "ttl": k.ttl,
        "value": value,
    })
}

/// Keys as text (behaviour 7).
pub fn export_keys(keys: &[KeyValue], format: ExportFormat) -> String {
    match format {
        ExportFormat::Json => {
            let v: Vec<_> = keys.iter().map(key_json).collect();
            serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
        }
        ExportFormat::Jsonl => keys
            .iter()
            .map(|k| key_json(k).to_string() + "\n")
            .collect(),
        ExportFormat::Csv => {
            let mut out = String::from("key,type,ttl,value\n");
            for k in keys {
                let j = key_json(k);
                let key = match &j["key"] {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let value = match &j["value"] {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let ttl = k.ttl.map(|t| t.to_string()).unwrap_or_default();
                out.push_str(&format!(
                    "{},{},{},{}\n",
                    csv_field(&key),
                    k.kind,
                    ttl,
                    csv_field(&value)
                ));
            }
            out
        }
    }
}

/// Writes `text` to `path`; an existing file is replaced only with `overwrite`.
pub fn write_export(path: &Path, text: &str, overwrite: bool) -> Result<(), StoreError> {
    if path.exists() && !overwrite {
        return Err(StoreError::Confirm(format!(
            "{} exists; confirm to overwrite it",
            path.display()
        )));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| StoreError::File(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, text).map_err(|e| StoreError::File(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    #[test]
    fn documents_as_json_lines_and_csv() {
        let docs = vec![
            doc! {"_id": 1, "name": "a, b", "dim": {"w": 2}},
            doc! {"_id": 2, "q": "say \"hi\""},
        ];
        let json = export_documents(&docs, ExportFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["dim"]["w"], 2);
        assert_eq!(
            export_documents(&docs, ExportFormat::Jsonl),
            "{\"_id\":1,\"name\":\"a, b\",\"dim\":{\"w\":2}}\n{\"_id\":2,\"q\":\"say \\\"hi\\\"\"}\n"
        );
        assert_eq!(
            export_documents(&docs, ExportFormat::Csv),
            "_id,name,dim,q\n1,\"a, b\",\"{\"\"w\"\":2}\",\n2,,,\"say \"\"hi\"\"\"\n"
        );
    }

    #[test]
    fn keys_as_json_lines_and_csv_with_binary_values() {
        let keys = vec![
            KeyValue {
                key: b"greeting".to_vec(),
                kind: "string".into(),
                ttl: Some(60),
                total: 5,
                data: ValueData::String(b"hello".to_vec()),
            },
            KeyValue {
                key: b"blob".to_vec(),
                kind: "string".into(),
                ttl: None,
                total: 2,
                data: ValueData::String(vec![0xff, 0x00]),
            },
            KeyValue {
                key: b"h".to_vec(),
                kind: "hash".into(),
                ttl: None,
                total: 1,
                data: ValueData::Pairs(vec![(b"f".to_vec(), b"v".to_vec())]),
            },
        ];
        let lines = export_keys(&keys, ExportFormat::Jsonl);
        assert_eq!(
            lines.lines().next().unwrap(),
            r#"{"key":"greeting","type":"string","ttl":60,"value":"hello"}"#
        );
        assert!(lines.contains(r#""value":{"base64":"/wA="}"#), "{lines}");
        assert_eq!(
            export_keys(&keys, ExportFormat::Csv),
            "key,type,ttl,value\ngreeting,string,60,hello\nblob,string,,\"{\"\"base64\"\":\"\"/wA=\"\"}\"\nh,hash,,\"[[\"\"f\"\",\"\"v\"\"]]\"\n"
        );
        let dir =
            std::env::temp_dir().join(format!("idecraft-nosql-export-{}", std::process::id()));
        let path = dir.join("keys.json");
        let _ = std::fs::remove_dir_all(&dir);
        write_export(&path, "[]", false).unwrap();
        assert!(
            write_export(&path, "[]", false)
                .unwrap_err()
                .to_string()
                .contains("exists")
        );
        write_export(&path, "[1]", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1]");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
