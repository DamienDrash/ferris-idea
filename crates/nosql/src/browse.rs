//! One level of the browser tree (`SPEC-0310` behaviour 5), shared by the Database tool window and
//! MCP `browse_store`.
//!
//! Paths: MongoDB `[]` databases, `[db]` collections, `[db, coll]` the groups "indexes" and
//! "fields", `[db, coll, "indexes"|"fields"]` their entries. Redis `[]` databases, `[db]` keys
//! and groups of the first page, `[db, prefix]` (prefix ending in `:`) the keys under it.

use serde::Serialize;

use crate::{
    MongoSession, RedisSession, StoreError, StoreProfile, System, completion::scan_pattern,
    key_tree,
};

/// Keys read per tree level at most (behaviour 5).
pub const KEY_PAGE: usize = 500;

/// One child in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowseItem {
    /// Name shown.
    pub label: String,
    /// Grey text (size, count, type, TTL).
    pub detail: String,
    /// Path of this item (to browse its children).
    pub path: Vec<String>,
    /// Whether it has children.
    pub expandable: bool,
    /// `database`, `collection`, `view`, `group`, `index`, `field`, `key`, `more`.
    pub kind: &'static str,
}

/// A level of the tree and the cursor of the next key page (Redis; 0 = complete).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowseLevel {
    /// Children.
    pub items: Vec<BrowseItem>,
    /// `SCAN` cursor to continue with; 0 when everything was listed.
    pub cursor: u64,
}

fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn ttl_text(ttl: Option<i64>) -> String {
    match ttl {
        None => String::new(),
        Some(s) => format!(" · TTL {s} s"),
    }
}

/// Children of `path` in the store of `profile` (connects with `password`).
pub async fn browse(
    profile: &StoreProfile,
    password: Option<idecraft_database::Secret>,
    path: &[String],
    cursor: u64,
) -> Result<BrowseLevel, StoreError> {
    match profile.system {
        System::Mongodb => {
            let s = MongoSession::connect(profile, password).await?;
            browse_mongo(&s, path)
                .await
                .map(|items| BrowseLevel { items, cursor: 0 })
        }
        System::Redis => {
            let mut s = RedisSession::connect(profile, password).await?;
            browse_redis(&mut s, path, cursor).await
        }
    }
}

/// [`browse`] on an open MongoDB session.
pub async fn browse_mongo(
    s: &MongoSession,
    path: &[String],
) -> Result<Vec<BrowseItem>, StoreError> {
    let child = |extra: &str| {
        let mut p = path.to_vec();
        p.push(extra.to_owned());
        p
    };
    Ok(match path {
        [] => s
            .databases()
            .await?
            .into_iter()
            .map(|d| BrowseItem {
                path: child(&d.name),
                detail: size(d.size),
                label: d.name,
                expandable: true,
                kind: "database",
            })
            .collect(),
        [db] => s
            .collections(db)
            .await?
            .into_iter()
            .map(|c| BrowseItem {
                path: child(&c.name),
                detail: match c.count {
                    Some(n) => format!("{n} documents"),
                    None => c.kind.to_owned(),
                },
                label: c.name,
                expandable: c.kind != "view",
                kind: if c.kind == "view" {
                    "view"
                } else {
                    "collection"
                },
            })
            .collect(),
        [_, _] => ["indexes", "fields"]
            .iter()
            .map(|g| BrowseItem {
                label: (*g).to_owned(),
                detail: String::new(),
                path: child(g),
                expandable: true,
                kind: "group",
            })
            .collect(),
        [db, coll, g] if g == "indexes" => s
            .indexes(db, coll)
            .await?
            .into_iter()
            .map(|i| {
                let mut detail = i.keys.clone();
                if i.unique {
                    detail.push_str(" · unique");
                }
                if let Some(t) = i.ttl {
                    detail.push_str(&format!(" · TTL {t} s"));
                }
                BrowseItem {
                    path: child(&i.name),
                    label: i.name,
                    detail,
                    expandable: false,
                    kind: "index",
                }
            })
            .collect(),
        [db, coll, g] if g == "fields" => {
            let fields = s.sample_fields(db, coll).await?;
            fields
                .into_iter()
                .map(|f| BrowseItem {
                    path: child(&f.path),
                    detail: format!("{} · in {} sampled", f.types.join(" | "), f.count),
                    label: f.path,
                    expandable: false,
                    kind: "field",
                })
                .collect()
        }
        _ => {
            return Err(StoreError::Edit(format!(
                "no such tree path: {}",
                path.join(" / ")
            )));
        }
    })
}

/// [`browse`] on an open Redis session: `SCAN` until a page of [`KEY_PAGE`] keys or the end.
pub async fn browse_redis(
    s: &mut RedisSession,
    path: &[String],
    cursor: u64,
) -> Result<BrowseLevel, StoreError> {
    let (db, prefix) = match path {
        [] => {
            let items = s
                .databases()
                .await?
                .into_iter()
                .map(|(n, keys)| BrowseItem {
                    label: format!("db{n}"),
                    detail: format!("{keys} keys"),
                    path: vec![n.to_string()],
                    expandable: true,
                    kind: "database",
                })
                .collect();
            return Ok(BrowseLevel { items, cursor: 0 });
        }
        [db] => (db, String::new()),
        [db, prefix] => (db, prefix.clone()),
        _ => {
            return Err(StoreError::Edit(format!(
                "no such tree path: {}",
                path.join(" / ")
            )));
        }
    };
    let db: u8 = db
        .parse()
        .map_err(|_| StoreError::Edit(format!("`{db}` is not a database number")))?;
    if db != s.db() {
        s.run(&format!("SELECT {db}"), None, false).await?;
    }
    let pattern = scan_pattern(&prefix);
    let mut keys = Vec::new();
    let mut next = cursor;
    for _ in 0..50 {
        let (c, page) = s.scan(&pattern, next, KEY_PAGE).await?;
        keys.extend(page);
        next = c;
        if next == 0 || keys.len() >= KEY_PAGE {
            break;
        }
    }
    let infos: std::collections::HashMap<String, (String, Option<i64>)> = keys
        .iter()
        .map(|k| {
            (
                String::from_utf8_lossy(&k.name).into_owned(),
                (k.kind.clone(), k.ttl),
            )
        })
        .collect();
    let names: Vec<String> = infos
        .keys()
        .filter_map(|k| k.strip_prefix(&prefix).map(str::to_owned))
        .collect();
    let mut items = Vec::new();
    for node in key_tree(&names, ':') {
        if !node.children.is_empty() {
            let group = format!("{prefix}{}:", node.label);
            items.push(BrowseItem {
                label: format!("{}:", node.label),
                detail: format!("{} keys", node.keys - usize::from(node.key.is_some())),
                path: vec![db.to_string(), group],
                expandable: true,
                kind: "group",
            });
        }
        if let Some(rest) = &node.key {
            let full = format!("{prefix}{rest}");
            let (kind, ttl) = infos.get(&full).cloned().unwrap_or_default();
            items.push(BrowseItem {
                label: node.label.clone(),
                detail: format!("{kind}{}", ttl_text(ttl)),
                path: vec![db.to_string(), full],
                expandable: false,
                kind: "key",
            });
        }
    }
    if next != 0 {
        items.push(BrowseItem {
            label: "more…".into(),
            detail: format!("cursor {next}"),
            path: path.to_vec(),
            expandable: false,
            kind: "more",
        });
    }
    Ok(BrowseLevel {
        items,
        cursor: next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_ttls() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(73_728), "72.0 KB");
        assert_eq!(ttl_text(Some(5)), " · TTL 5 s");
        assert_eq!(ttl_text(None), "");
    }
}
