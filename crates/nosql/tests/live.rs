//! SPEC-0310 AK-5, SPEC-0311 AK-6, SPEC-0312 AK-6 against real servers. Start them with
//! `nosql.sh start` of the reference environment and export what `nosql.sh env` prints:
//! `FERRIS_IDEA_TEST_MONGO` / `FERRIS_IDEA_TEST_REDIS` (URLs without password) and
//! `FERRIS_IDEA_TEST_MONGO_PASSWORD_FILE` / `FERRIS_IDEA_TEST_REDIS_PASSWORD_FILE`. Without the URLs these
//! tests pass without doing anything.

use std::collections::BTreeSet;

use ferris_idea_database::{Permission, Secret, default_permissions};
use ferris_idea_nosql::bson::Bson;
use ferris_idea_nosql::{
    ExportFormat, MongoReply, MongoSession, RedisEdit, RedisSession, Reply, StoreError,
    StoreProfile, ValueData, diff_documents, export_documents, plan_update,
};

fn password(var: &str) -> Option<Secret> {
    let path = std::env::var(var).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    Some(Secret::new(text.trim()))
}

async fn mongo(read_only: bool) -> Option<MongoSession> {
    let url = ferris_idea_core::product_var("FERRIS_IDEA_TEST_MONGO").ok()?;
    let mut p = StoreProfile::from_url("live mongo", &url).unwrap();
    p.read_only = read_only;
    Some(
        MongoSession::connect(&p, password("FERRIS_IDEA_TEST_MONGO_PASSWORD_FILE"))
            .await
            .unwrap(),
    )
}

async fn redis(read_only: bool) -> Option<RedisSession> {
    let url = ferris_idea_core::product_var("FERRIS_IDEA_TEST_REDIS").ok()?;
    let mut p = StoreProfile::from_url("live redis", &url).unwrap();
    p.read_only = read_only;
    Some(
        RedisSession::connect(&p, password("FERRIS_IDEA_TEST_REDIS_PASSWORD_FILE"))
            .await
            .unwrap(),
    )
}

fn docs(reply: MongoReply) -> (Vec<ferris_idea_nosql::bson::Document>, bool) {
    match reply {
        MongoReply::Documents {
            documents, more, ..
        } => (documents, more),
        other => panic!("expected documents, got {other:?}"),
    }
}

#[tokio::test]
async fn mongo_tree_crud_paging_and_edit_conflict() {
    let Some(mut s) = mongo(false).await else {
        return;
    };
    let db = format!("ferris_idea_live_{}", std::process::id());
    s.run(&format!("use {db}"), 0, 50, false, None)
        .await
        .unwrap();
    let items: Vec<String> = (0..120)
        .map(|i| {
            format!(
                "{{_id: {i}, name: 'item {i}', price: {}, tags: ['t{}'], dim: {{w: {i}}}}}",
                i % 7,
                i % 3
            )
        })
        .collect();
    let insert = format!("db.items.insertMany([{}])", items.join(","));
    match s.run(&insert, 0, 50, false, None).await.unwrap() {
        MongoReply::Written(w) => assert_eq!(w.get_i64("insertedCount").unwrap(), 120),
        other => panic!("{other:?}"),
    }

    // Tree.
    assert!(s.databases().await.unwrap().iter().any(|d| d.name == db));
    let colls = s.collections(&db).await.unwrap();
    assert_eq!(colls.len(), 1);
    assert_eq!(
        (colls[0].name.as_str(), colls[0].kind, colls[0].count),
        ("items", "collection", Some(120))
    );
    s.run(
        "db.items.createIndex({price: -1}, {name: 'by_price', unique: false})",
        0,
        50,
        false,
        None,
    )
    .await
    .unwrap();
    let indexes = s.indexes(&db, "items").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|i| i.name == "by_price" && i.keys == r#"{"price":-1}"#),
        "{indexes:?}"
    );
    let fields = s.sample_fields(&db, "items").await.unwrap();
    let paths: Vec<&str> = fields.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"dim.w") && paths.contains(&"tags"),
        "{paths:?}"
    );

    // The browser tree (SPEC-0310 behaviour 5).
    let level = ferris_idea_nosql::browse_mongo(&s, &[]).await.unwrap();
    assert!(level.iter().any(|i| i.label == db && i.kind == "database"));
    let level = ferris_idea_nosql::browse_mongo(&s, std::slice::from_ref(&db))
        .await
        .unwrap();
    assert_eq!(level[0].label, "items");
    assert_eq!(level[0].detail, "120 documents");
    let path = vec![db.clone(), "items".to_owned(), "indexes".to_owned()];
    let level = ferris_idea_nosql::browse_mongo(&s, &path).await.unwrap();
    assert!(level.iter().any(|i| i.label == "by_price"), "{level:?}");
    let path = vec![db.clone(), "items".to_owned(), "fields".to_owned()];
    let level = ferris_idea_nosql::browse_mongo(&s, &path).await.unwrap();
    assert!(
        level
            .iter()
            .any(|i| i.label == "price" && i.detail.starts_with("int32")),
        "{level:?}"
    );

    // Paging with sort, limit and skip.
    let (page0, more) = docs(
        s.run("db.items.find({}).sort({_id: 1})", 0, 50, false, None)
            .await
            .unwrap(),
    );
    assert_eq!((page0.len(), more), (50, true));
    let (page2, more) = docs(
        s.run("db.items.find({}).sort({_id: 1})", 2, 50, false, None)
            .await
            .unwrap(),
    );
    assert_eq!((page2.len(), more), (20, false));
    assert_eq!(page2[0].get_i32("_id").unwrap(), 100);
    let (limited, more) = docs(
        s.run(
            "db.items.find({price: {$gte: 3}}, {name: 1}).sort({_id: -1}).skip(1).limit(5)",
            0,
            50,
            false,
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!((limited.len(), more), (5, false));
    assert!(limited[0].get("price").is_none(), "projection");
    let (one, _) = docs(
        s.run("db.items.findOne({_id: 7})", 0, 50, false, None)
            .await
            .unwrap(),
    );
    assert_eq!(one[0].get_str("name").unwrap(), "item 7");

    match s
        .run("db.items.countDocuments({price: 0})", 0, 50, false, None)
        .await
        .unwrap()
    {
        MongoReply::Value(Bson::Int64(n)) => assert_eq!(n, 18),
        other => panic!("{other:?}"),
    }
    match s
        .run("db.items.distinct('tags')", 0, 50, false, None)
        .await
        .unwrap()
    {
        MongoReply::Value(Bson::Array(v)) => assert_eq!(v.len(), 3),
        other => panic!("{other:?}"),
    }
    let (groups, _) = docs(
        s.run(
            "db.items.aggregate([{$group: {_id: '$price', n: {$sum: 1}}}, {$sort: {_id: 1}}])",
            0,
            50,
            false,
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(groups.len(), 7);

    // Edit through the plan; a concurrent change is detected.
    let original = one[0].clone();
    let mut edited = original.clone();
    edited.insert("name", "pencil");
    let edit = plan_update("items", &original, &diff_documents(&original, &edited)).unwrap();
    s.apply_edit(&edit, None).await.unwrap();
    let err = s.apply_edit(&edit, None).await.unwrap_err().to_string();
    assert!(err.contains("changed since it was read"), "{err}");

    // Export streams every page.
    let all = s
        .read_all("db.items.find({}).sort({_id: 1})", None)
        .await
        .unwrap();
    assert_eq!(all.len(), 120);
    assert_eq!(
        export_documents(&all, ExportFormat::Jsonl).lines().count(),
        120
    );

    // Agent rights and confirmation.
    let read: BTreeSet<Permission> = default_permissions();
    let err = s
        .run("db.items.insertOne({_id: 999})", 0, 50, false, Some(&read))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::Refused {
                needed: Permission::ModifyData,
                ..
            }
        ),
        "{err}"
    );
    let err = s
        .run("db.items.deleteMany({})", 0, 50, false, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("all 120 documents of `items`"), "{err}");
    match s
        .run("db.items.deleteMany({})", 0, 50, true, None)
        .await
        .unwrap()
    {
        MongoReply::Written(w) => assert_eq!(w.get_i64("deletedCount").unwrap(), 120),
        other => panic!("{other:?}"),
    }
    s.run("db.runCommand({dropDatabase: 1})", 0, 50, false, None)
        .await
        .unwrap();

    // Server errors keep their code; parse errors name the offset.
    let err = s
        .run("db.items.find({$bad: 1})", 0, 50, false, None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, StoreError::Server { code: Some(_), .. }),
        "{err}"
    );
    let err = s
        .run("db.items.find({a: x})", 0, 50, false, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("at 18:"), "{err}");
}

#[tokio::test]
async fn mongo_read_only_refuses_writes() {
    let Some(mut s) = mongo(true).await else {
        return;
    };
    let err = s
        .run("db.x.insertOne({a: 1})", 0, 50, false, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("read-only"), "{err}");
    let err = s
        .run("db.runCommand({dropDatabase: 1})", 0, 50, false, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("read-only"), "{err}");
    assert!(
        s.run("db.runCommand({ping: 1})", 0, 50, false, None)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn redis_types_ttl_pages_and_rights() {
    let Some(mut s) = redis(false).await else {
        return;
    };
    let p = format!("ferris-idea:live:{}", std::process::id());
    let script = format!(
        "SET {p}:s \"hello world\"\nSET {p}:j '{{\"a\": 1}}'\nSET {p}:bin \"\\xff\\x00\"\n\
         HSET {p}:h f1 v1 f2 v2\nRPUSH {p}:l a b c\nSADD {p}:set x y\nZADD {p}:z 1 one 2.5 two\n\
         XADD {p}:x 1-1 k v\nEXPIRE {p}:s 100"
    );
    let replies = s.run(&script, None, false).await.unwrap();
    assert_eq!(replies.len(), 9);
    assert!(
        replies.iter().all(|(_, r)| !matches!(r, Reply::Error(_))),
        "{replies:?}"
    );

    let v = s.read(format!("{p}:s").as_bytes()).await.unwrap();
    assert_eq!((v.kind.as_str(), v.total), ("string", 11));
    assert!(
        matches!(v.ttl, Some(t) if (1..=100).contains(&t)),
        "{:?}",
        v.ttl
    );
    assert_eq!(v.data, ValueData::String(b"hello world".to_vec()));
    let v = s.read(format!("{p}:bin").as_bytes()).await.unwrap();
    assert_eq!(v.data, ValueData::String(vec![0xff, 0x00]));
    assert_eq!(
        ferris_idea_nosql::bytes_display(&[0xff, 0x00]),
        "0xff00 (2 bytes)"
    );
    let v = s.read(format!("{p}:h").as_bytes()).await.unwrap();
    assert_eq!((v.kind.as_str(), v.total, v.shown()), ("hash", 2, 2));
    let v = s.read(format!("{p}:l").as_bytes()).await.unwrap();
    assert_eq!(
        v.data,
        ValueData::Items(vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()])
    );
    let v = s.read(format!("{p}:set").as_bytes()).await.unwrap();
    assert_eq!(v.data, ValueData::Items(vec![b"x".to_vec(), b"y".to_vec()]));
    let v = s.read(format!("{p}:z").as_bytes()).await.unwrap();
    assert_eq!(
        v.data,
        ValueData::Scored(vec![(b"one".to_vec(), 1.0), (b"two".to_vec(), 2.5)])
    );
    let v = s.read(format!("{p}:x").as_bytes()).await.unwrap();
    assert_eq!(
        v.data,
        ValueData::Stream(vec![("1-1".into(), vec![(b"k".to_vec(), b"v".to_vec())])])
    );

    // Edits: TTL removed, rename refused on a clash, list element set.
    let key = format!("{p}:s").into_bytes();
    s.apply(
        &RedisEdit::Ttl {
            key: key.clone(),
            seconds: None,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(s.read(&key).await.unwrap().ttl, None);
    let err = s
        .apply(
            &RedisEdit::Rename {
                key: key.clone(),
                to: format!("{p}:h").into_bytes(),
            },
            None,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("exists; nothing was renamed"), "{err}");
    s.apply(
        &RedisEdit::ListSet {
            key: format!("{p}:l").into_bytes(),
            index: 1,
            value: b"B".to_vec(),
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        s.read(format!("{p}:l").as_bytes()).await.unwrap().data,
        ValueData::Items(vec![b"a".to_vec(), b"B".to_vec(), b"c".to_vec()])
    );

    // A wrong-type reply stops the run.
    let replies = s
        .run(&format!("GET {p}:h\nGET {p}:s"), None, false)
        .await
        .unwrap();
    assert_eq!(replies.len(), 1);
    assert!(
        matches!(&replies[0].1, Reply::Error(e) if e.starts_with("WRONGTYPE")),
        "{replies:?}"
    );

    // SCAN pages with "more".
    let many: Vec<String> = (0..1200)
        .map(|i| format!("MSET {p}:page:{i} {i}"))
        .collect();
    s.run(&many.join("\n"), None, false).await.unwrap();
    let pattern = format!("{p}:page:*");
    let (mut cursor, mut seen, mut pages) = (0, 0, 0);
    loop {
        let (next, keys) = s.scan(&pattern, cursor, 500).await.unwrap();
        assert!(keys.len() <= 1200);
        seen += keys.len();
        pages += 1;
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    assert!(pages > 1, "more than one page");
    assert_eq!(seen, 1200);
    assert!(
        s.databases()
            .await
            .unwrap()
            .iter()
            .any(|(db, n)| *db == s.db() && *n >= 1200)
    );
    let names = s.key_names(&format!("{p}:page:11")).await.unwrap();
    assert!(names.iter().all(|n| n.starts_with(&format!("{p}:page:11"))));

    // The browser tree groups keys by `:` and pages with a cursor.
    let db = s.db().to_string();
    let level = ferris_idea_nosql::browse_redis(&mut s, &[db.clone(), format!("{p}:")], 0)
        .await
        .unwrap();
    assert!(
        level
            .items
            .iter()
            .any(|i| i.label == "page:" && i.kind == "group"),
        "{:?}",
        level.items
    );
    assert!(
        level
            .items
            .iter()
            .filter(|i| i.kind == "key")
            .all(|i| !i.detail.is_empty()),
        "keys show their type: {:?}",
        level.items
    );
    let level = ferris_idea_nosql::browse_redis(&mut s, &[db.clone(), format!("{p}:page:")], 0)
        .await
        .unwrap();
    assert!(level.cursor != 0, "1200 keys need more than one page");
    assert_eq!(level.items.last().unwrap().kind, "more");

    // Rights and confirmation.
    let read = default_permissions();
    let err = s
        .run(&format!("DEL {p}:s"), Some(&read), false)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::Refused {
                needed: Permission::ModifyData,
                ..
            }
        ),
        "{err}"
    );
    let err = s.run("FLUSHDB", None, false).await.unwrap_err().to_string();
    assert!(
        err.contains("FLUSHDB deletes database") && err.contains("confirm"),
        "{err}"
    );

    // Clean up.
    let (mut cursor, mut keys) = (0, Vec::new());
    loop {
        let (next, page) = s.scan(&format!("{p}:*"), cursor, 500).await.unwrap();
        keys.extend(page.into_iter().map(|k| String::from_utf8(k.name).unwrap()));
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    for chunk in keys.chunks(200) {
        s.run(&format!("UNLINK {}", chunk.join(" ")), None, false)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn redis_read_only_refuses_writes() {
    let Some(mut s) = redis(true).await else {
        return;
    };
    let err = s.run("SET a 1", None, false).await.unwrap_err().to_string();
    assert!(err.contains("read-only"), "{err}");
    let err = s
        .run("CONFIG SET maxmemory 1", None, false)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("read-only"), "{err}");
    assert!(s.run("PING", None, false).await.is_ok());
}

#[tokio::test]
async fn test_connection_names_the_server_version() {
    for (var, pw) in [
        (
            "FERRIS_IDEA_TEST_MONGO",
            "FERRIS_IDEA_TEST_MONGO_PASSWORD_FILE",
        ),
        (
            "FERRIS_IDEA_TEST_REDIS",
            "FERRIS_IDEA_TEST_REDIS_PASSWORD_FILE",
        ),
    ] {
        let Ok(url) = std::env::var(var) else {
            continue;
        };
        let p = StoreProfile::from_url("live", &url).unwrap();
        let note = ferris_idea_nosql::test_store(&p, password(pw))
            .await
            .unwrap();
        assert!(
            note.starts_with("Connected: ") && note.contains("8."),
            "{note}"
        );
        let err = ferris_idea_nosql::test_store(&p, Some(Secret::new("wrong")))
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::Connect { .. }), "{err}");
    }
}
