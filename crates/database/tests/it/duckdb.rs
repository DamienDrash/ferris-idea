//! SPEC-0304 behaviour 4 against the bundled DuckDB engine: a database file of its own per test, no
//! server needed, so these run everywhere.

use ferris_idea_database::{
    ConnectionProfile, Credentials, Dialect, OnError, Outcome, Session, current_schema, explain,
    introspect, list_schemas, test_connection, view_definitions,
};

fn temp_file(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-duckdb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("shop.duckdb")
}

/// Creates an empty DuckDB file the way a user would (any DuckDB client).
fn create(path: &std::path::Path) {
    duckdb::Connection::open(path).unwrap();
}

fn profile(path: &std::path::Path) -> ConnectionProfile {
    ConnectionProfile::from_url("shop", &format!("duckdb:{}", path.display())).unwrap()
}

fn rows(o: &Outcome) -> Vec<Vec<Option<String>>> {
    match o {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn missing_file_is_refused_and_not_created() {
    let path = temp_file("missing");
    let err = Session::connect(&profile(&path), &Credentials::NotNeeded)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("does not exist"), "{err}");
    assert!(!path.exists());
}

#[tokio::test]
async fn scripts_values_catalogue_and_plans() {
    let path = temp_file("script");
    create(&path);
    let p = profile(&path);
    assert_eq!(p.dialect, Dialect::DuckDb);
    assert_eq!(p.to_url(), format!("duckdb:{}", path.display()));
    assert_eq!(
        test_connection(&p, &Credentials::NotNeeded).await.unwrap(),
        "Connected to shop"
    );
    let s = Session::connect(&p, &Credentials::NotNeeded).await.unwrap();
    assert_eq!(s.dialect(), Dialect::DuckDb);
    let run = s
        .run_script(
            "create table parent (id integer primary key, name varchar not null);\n\
             create table child (id integer primary key, parent_id integer not null references parent (id), \
               note varchar default 'x', tags varchar[]);\n\
             create index child_note on child (note);\n\
             create view child_ids as select id from child;\n\
             insert into parent values (1, 'a'), (2, 'b');\n\
             insert into child values (10, 1, 'n', ['red', 'blue']), (11, 2, null, null);\n\
             select id, note, tags, date '2026-11-05' as d, 1.5::double as f from child order by id;\n\
             select $body$a;b$body$ as dollar, null as nothing",
            OnError::Stop,
        )
        .await;
    assert_eq!(run.outcomes.len(), 8, "{:?}", run.outcomes);
    assert_eq!(run.outcomes[4].outcome, Outcome::UpdateCount(2));
    assert_eq!(
        rows(&run.outcomes[6].outcome),
        [
            [
                Some("10".into()),
                Some("n".into()),
                Some("[red, blue]".into()),
                Some("2026-11-05".into()),
                Some("1.5".into())
            ],
            [
                Some("11".into()),
                None,
                None,
                Some("2026-11-05".into()),
                Some("1.5".into())
            ]
        ]
    );
    assert_eq!(
        rows(&run.outcomes[7].outcome),
        [[Some("a;b".to_owned()), None]]
    );
    assert_eq!(current_schema(&s).await.unwrap(), "main");
    assert!(
        list_schemas(&s)
            .await
            .unwrap()
            .iter()
            .any(|x| x.name == "main" && x.current)
    );
    let tables = introspect(&s).await.unwrap();
    let child = tables.iter().find(|t| t.name == "child").unwrap();
    assert_eq!(child.primary_key, ["id"]);
    let columns: Vec<&str> = child.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["id", "parent_id", "note", "tags"]);
    assert!(!child.columns[1].nullable);
    assert_eq!(child.foreign_keys.len(), 1, "{:?}", child.foreign_keys);
    assert_eq!(child.foreign_keys[0].referenced_table, "parent");
    assert_eq!(child.foreign_keys[0].referenced_columns, ["id"]);
    assert!(
        child
            .indexes
            .iter()
            .any(|i| i.name == "child_note" && i.columns == ["note"]),
        "{:?}",
        child.indexes
    );
    assert!(
        tables
            .iter()
            .any(|t| t.name == "child_ids" && t.kind == "view")
    );
    let views = view_definitions(&s).await.unwrap();
    assert!(
        views
            .iter()
            .any(|v| v.name == "child_ids" && v.definition.to_lowercase().starts_with("select")),
        "{views:?}"
    );
    let plan = explain(&s, "select * from child where parent_id = 1", false)
        .await
        .unwrap();
    assert!(!plan.nodes.is_empty(), "{plan:?}");
    let measured = explain(&s, "update child set note = 'm' where id = 10", true)
        .await
        .unwrap();
    assert!(measured.analyzed && measured.rolled_back);
    let note = s
        .run_script("select note from child where id = 10", OnError::Stop)
        .await;
    assert_eq!(
        rows(&note.outcomes[0].outcome),
        [[Some("n".to_owned())]],
        "measuring was rolled back"
    );
    let bad = s.run_script("select nope from child", OnError::Stop).await;
    assert!(
        matches!(&bad.outcomes[0].outcome, Outcome::Error(e) if e.message.starts_with("DuckDB:"))
    );
}

#[tokio::test]
async fn read_only_profiles_cannot_write() {
    let path = temp_file("readonly");
    create(&path);
    let mut p = profile(&path);
    p.read_only = true;
    let s = Session::connect(&p, &Credentials::NotNeeded).await.unwrap();
    let run = s.run_script("create table t (a int)", OnError::Stop).await;
    assert!(
        matches!(&run.outcomes[0].outcome, Outcome::Error(_)),
        "{:?}",
        run.outcomes
    );
}
