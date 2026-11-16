//! Edits with a preview (`SPEC-0311` behaviour 7, `SPEC-0312` behaviour 4).
//!
//! A MongoDB edit only touches the changed paths and filters on their old values, so a document
//! changed since it was read is never overwritten (IMP-0291). A Redis edit is an argument list,
//! never a command line with interpolated values.

use mongodb::bson::{Bson, Document, doc};

use crate::StoreError;
use crate::view::compact;

/// One change to a document field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldChange {
    /// Set `path` (dotted) to `new`; `old` is the value read (`None`: field was missing).
    Set {
        /// Dotted path.
        path: String,
        /// Value read.
        old: Option<Bson>,
        /// New value.
        new: Bson,
    },
    /// Remove `path`, which held `old`.
    Unset {
        /// Dotted path.
        path: String,
        /// Value read.
        old: Bson,
    },
}

/// A planned MongoDB write on one document.
#[derive(Debug, Clone, PartialEq)]
pub struct MongoEdit {
    /// Collection.
    pub collection: String,
    /// Filter: `_id` plus the old values of the changed paths.
    pub filter: Document,
    /// `$set`/`$unset` update, or the replacement document.
    pub update: Document,
    /// Whether `update` replaces the document.
    pub replace: bool,
}

impl MongoEdit {
    /// The command as the console would show it.
    pub fn preview(&self) -> String {
        let method = if self.replace {
            "replaceOne"
        } else {
            "updateOne"
        };
        format!(
            "db.{}.{method}({}, {})",
            self.collection,
            compact(&Bson::Document(self.filter.clone())),
            compact(&Bson::Document(self.update.clone()))
        )
    }
}

fn id_of(original: &Document) -> Result<Bson, StoreError> {
    original
        .get("_id")
        .cloned()
        .ok_or_else(|| StoreError::Edit("documents without `_id` cannot be edited".into()))
}

/// Plans `updateOne` for `changes` to `original` (behaviour 7).
pub fn plan_update(
    collection: &str,
    original: &Document,
    changes: &[FieldChange],
) -> Result<MongoEdit, StoreError> {
    let id = id_of(original)?;
    if changes.is_empty() {
        return Err(StoreError::Edit("nothing changed".into()));
    }
    let mut filter = doc! {"_id": id};
    let mut set = Document::new();
    let mut unset = Document::new();
    for change in changes {
        match change {
            FieldChange::Set { path, old, new } => {
                if path == "_id" || path.starts_with("_id.") {
                    return Err(StoreError::Edit("`_id` cannot be changed".into()));
                }
                filter.insert(
                    path.clone(),
                    match old {
                        Some(v) => v.clone(),
                        None => Bson::Document(doc! {"$exists": false}),
                    },
                );
                set.insert(path.clone(), new.clone());
            }
            FieldChange::Unset { path, old } => {
                if path == "_id" {
                    return Err(StoreError::Edit("`_id` cannot be removed".into()));
                }
                filter.insert(path.clone(), old.clone());
                unset.insert(path.clone(), "");
            }
        }
    }
    let mut update = Document::new();
    if !set.is_empty() {
        update.insert("$set", set);
    }
    if !unset.is_empty() {
        update.insert("$unset", unset);
    }
    Ok(MongoEdit {
        collection: collection.to_owned(),
        filter,
        update,
        replace: false,
    })
}

/// Plans `replaceOne` of `original` by `replacement`; the filter is the whole original, so any
/// change by someone else makes it match nothing.
pub fn plan_replace(
    collection: &str,
    original: &Document,
    replacement: &Document,
) -> Result<MongoEdit, StoreError> {
    let id = id_of(original)?;
    if replacement.get("_id").is_some_and(|r| *r != id) {
        return Err(StoreError::Edit("`_id` cannot be changed".into()));
    }
    let mut update = replacement.clone();
    update.remove("_id");
    Ok(MongoEdit {
        collection: collection.to_owned(),
        filter: original.clone(),
        update,
        replace: true,
    })
}

/// The changes that turn `original` into `edited`, nested documents path by path.
pub fn diff_documents(original: &Document, edited: &Document) -> Vec<FieldChange> {
    let mut out = Vec::new();
    diff_into("", original, edited, &mut out);
    out
}

fn diff_into(prefix: &str, a: &Document, b: &Document, out: &mut Vec<FieldChange>) {
    let path = |k: &str| {
        if prefix.is_empty() {
            k.to_owned()
        } else {
            format!("{prefix}.{k}")
        }
    };
    for (k, new) in b {
        match a.get(k) {
            Some(Bson::Document(old_d)) if matches!(new, Bson::Document(_)) => {
                if let Bson::Document(new_d) = new {
                    diff_into(&path(k), old_d, new_d, out);
                }
            }
            Some(old) if old == new => {}
            old => out.push(FieldChange::Set {
                path: path(k),
                old: old.cloned(),
                new: new.clone(),
            }),
        }
    }
    for (k, old) in a {
        if !b.contains_key(k) {
            out.push(FieldChange::Unset {
                path: path(k),
                old: old.clone(),
            });
        }
    }
}

/// A Redis edit (behaviour 4).
#[derive(Debug, Clone, PartialEq)]
pub enum RedisEdit {
    /// `SET key value KEEPTTL`
    SetString {
        /// Key.
        key: Vec<u8>,
        /// Value.
        value: Vec<u8>,
    },
    /// `EXPIRE key seconds` or `PERSIST key` (`None`).
    Ttl {
        /// Key.
        key: Vec<u8>,
        /// Seconds; `None` removes the TTL.
        seconds: Option<u64>,
    },
    /// `RENAMENX key new` — refused when `new` exists.
    Rename {
        /// Key.
        key: Vec<u8>,
        /// New name.
        to: Vec<u8>,
    },
    /// `UNLINK key`
    Delete {
        /// Key.
        key: Vec<u8>,
    },
    /// `HSET key field value`
    HashSet {
        /// Key.
        key: Vec<u8>,
        /// Field.
        field: Vec<u8>,
        /// Value.
        value: Vec<u8>,
    },
    /// `HDEL key field`
    HashRemove {
        /// Key.
        key: Vec<u8>,
        /// Field.
        field: Vec<u8>,
    },
    /// `LSET key index value`
    ListSet {
        /// Key.
        key: Vec<u8>,
        /// Index.
        index: i64,
        /// Value.
        value: Vec<u8>,
    },
    /// `RPUSH key value`
    ListAppend {
        /// Key.
        key: Vec<u8>,
        /// Value.
        value: Vec<u8>,
    },
    /// `LREM key 1 value`
    ListRemove {
        /// Key.
        key: Vec<u8>,
        /// Value.
        value: Vec<u8>,
    },
    /// `SADD key member`
    SetAdd {
        /// Key.
        key: Vec<u8>,
        /// Member.
        member: Vec<u8>,
    },
    /// `SREM key member`
    SetRemove {
        /// Key.
        key: Vec<u8>,
        /// Member.
        member: Vec<u8>,
    },
    /// `ZADD key score member`
    ZsetAdd {
        /// Key.
        key: Vec<u8>,
        /// Member.
        member: Vec<u8>,
        /// Score.
        score: f64,
    },
    /// `ZREM key member`
    ZsetRemove {
        /// Key.
        key: Vec<u8>,
        /// Member.
        member: Vec<u8>,
    },
}

impl RedisEdit {
    /// The command as an argument list.
    pub fn args(&self) -> Vec<Vec<u8>> {
        let b = |s: &str| s.as_bytes().to_vec();
        match self {
            RedisEdit::SetString { key, value } => {
                vec![b("SET"), key.clone(), value.clone(), b("KEEPTTL")]
            }
            RedisEdit::Ttl {
                key,
                seconds: Some(s),
            } => vec![b("EXPIRE"), key.clone(), b(&s.to_string())],
            RedisEdit::Ttl { key, seconds: None } => vec![b("PERSIST"), key.clone()],
            RedisEdit::Rename { key, to } => vec![b("RENAMENX"), key.clone(), to.clone()],
            RedisEdit::Delete { key } => vec![b("UNLINK"), key.clone()],
            RedisEdit::HashSet { key, field, value } => {
                vec![b("HSET"), key.clone(), field.clone(), value.clone()]
            }
            RedisEdit::HashRemove { key, field } => vec![b("HDEL"), key.clone(), field.clone()],
            RedisEdit::ListSet { key, index, value } => {
                vec![b("LSET"), key.clone(), b(&index.to_string()), value.clone()]
            }
            RedisEdit::ListAppend { key, value } => vec![b("RPUSH"), key.clone(), value.clone()],
            RedisEdit::ListRemove { key, value } => {
                vec![b("LREM"), key.clone(), b("1"), value.clone()]
            }
            RedisEdit::SetAdd { key, member } => vec![b("SADD"), key.clone(), member.clone()],
            RedisEdit::SetRemove { key, member } => vec![b("SREM"), key.clone(), member.clone()],
            RedisEdit::ZsetAdd { key, member, score } => vec![
                b("ZADD"),
                key.clone(),
                b(&score.to_string()),
                member.clone(),
            ],
            RedisEdit::ZsetRemove { key, member } => vec![b("ZREM"), key.clone(), member.clone()],
        }
    }

    /// The command as `redis-cli` would take it (quoted where needed).
    pub fn preview(&self) -> String {
        self.args()
            .iter()
            .map(|a| {
                let s = String::from_utf8_lossy(a);
                if !a.is_empty()
                    && a.iter()
                        .all(|c| c.is_ascii_graphic() && *c != b'"' && *c != b'\'')
                {
                    s.into_owned()
                } else {
                    let mut q = String::from("\"");
                    for &c in a {
                        match c {
                            b'"' => q.push_str("\\\""),
                            b'\\' => q.push_str("\\\\"),
                            b'\n' => q.push_str("\\n"),
                            0x20..=0x7e => q.push(c as char),
                            _ => q.push_str(&format!("\\x{c:02x}")),
                        }
                    }
                    q.push('"');
                    q
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_sets_only_changed_paths_and_filters_on_old_values() {
        let original = doc! {"_id": 7, "name": "pen", "dim": {"w": 1, "h": 2}, "old": true};
        let edited = doc! {"_id": 7, "name": "pencil", "dim": {"w": 1, "h": 3}, "colour": "red"};
        let changes = diff_documents(&original, &edited);
        let edit = plan_update("items", &original, &changes).unwrap();
        assert_eq!(
            edit.filter,
            doc! {"_id": 7, "name": "pen", "dim.h": 2, "colour": {"$exists": false}, "old": true}
        );
        assert_eq!(
            edit.update,
            doc! {"$set": {"name": "pencil", "dim.h": 3, "colour": "red"}, "$unset": {"old": ""}}
        );
        assert_eq!(
            edit.preview(),
            r#"db.items.updateOne({"_id":7,"name":"pen","dim.h":2,"colour":{"$exists":false},"old":true}, {"$set":{"name":"pencil","dim.h":3,"colour":"red"},"$unset":{"old":""}})"#
        );
    }

    #[test]
    fn replace_filters_on_the_whole_original_and_ids_are_kept() {
        let original = doc! {"_id": 1, "a": 1};
        let edit = plan_replace("c", &original, &doc! {"_id": 1, "b": 2}).unwrap();
        assert_eq!(edit.filter, original);
        assert_eq!(edit.update, doc! {"b": 2});
        assert!(edit.replace);
        assert!(plan_replace("c", &original, &doc! {"_id": 2}).is_err());
    }

    #[test]
    fn documents_without_id_and_id_changes_are_refused() {
        let err = plan_update("c", &doc! {"a": 1}, &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("without `_id`"), "{err}");
        let change = FieldChange::Set {
            path: "_id".into(),
            old: Some(Bson::Int32(1)),
            new: Bson::Int32(2),
        };
        assert!(plan_update("c", &doc! {"_id": 1}, &[change]).is_err());
        assert!(plan_update("c", &doc! {"_id": 1}, &[]).is_err());
    }

    #[test]
    fn redis_edits_are_argument_lists() {
        let k = b"user:1".to_vec();
        assert_eq!(
            RedisEdit::SetString {
                key: k.clone(),
                value: b"a b".to_vec()
            }
            .preview(),
            "SET user:1 \"a b\" KEEPTTL"
        );
        assert_eq!(
            RedisEdit::Ttl {
                key: k.clone(),
                seconds: Some(60)
            }
            .preview(),
            "EXPIRE user:1 60"
        );
        assert_eq!(
            RedisEdit::Ttl {
                key: k.clone(),
                seconds: None
            }
            .preview(),
            "PERSIST user:1"
        );
        assert_eq!(
            RedisEdit::Rename {
                key: k.clone(),
                to: b"user:2".to_vec()
            }
            .args()[0],
            b"RENAMENX".to_vec()
        );
        assert_eq!(
            RedisEdit::ListRemove {
                key: k.clone(),
                value: vec![0xff]
            }
            .preview(),
            "LREM user:1 1 \"\\xff\""
        );
        assert_eq!(
            RedisEdit::ZsetAdd {
                key: k,
                member: b"m".to_vec(),
                score: 2.5
            }
            .preview(),
            "ZADD user:1 2.5 m"
        );
    }
}
