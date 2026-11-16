//! SPEC-0116 against a real SQL Server. Set `IDECRAFT_TEST_SQLSERVER` to a SPEC-0008 URL of the
//! reference database (e.g. `sqlserver://idecraft@127.0.0.1:1433/inventory`) and the password in
//! `IDECRAFT_TEST_SQLSERVER_PASSWORD` or a file named by `IDECRAFT_TEST_SQLSERVER_PASSWORD_FILE`
//! (`mssql.sh` of the reference environment); without them these tests pass without doing anything.
//! Tests that create objects use their own `ide_*` names and drop them again.

use idecraft_database::{
    ConnectionProfile, Credentials, Dialect, OnError, Outcome, Secret, Session, explain, introspect,
};

async fn session(read_only: bool) -> Option<Session> {
    let url = std::env::var("IDECRAFT_TEST_SQLSERVER").ok()?;
    let password = std::env::var("IDECRAFT_TEST_SQLSERVER_PASSWORD")
        .ok()
        .or_else(|| {
            let file = std::env::var("IDECRAFT_TEST_SQLSERVER_PASSWORD_FILE").ok()?;
            Some(std::fs::read_to_string(file).ok()?.trim().to_owned())
        })?;
    let mut profile = ConnectionProfile::from_url("reference", &url).unwrap();
    profile.read_only = read_only;
    Some(
        Session::connect(&profile, &Credentials::Available(Secret::new(password)))
            .await
            .unwrap(),
    )
}

fn rows(o: &Outcome) -> Vec<Vec<Option<String>>> {
    match o {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn reference_counts_and_dialect() {
    let Some(s) = session(false).await else {
        return;
    };
    assert_eq!(s.dialect(), Dialect::SqlServer);
    let run = s
        .run_script(
            "select count(*), sum(delta) from movement;\nselect [city] from warehouse where city = N'Köln'\nGO\nselect name from item where sku = 'B-2'",
            OnError::Stop,
        )
        .await;
    assert_eq!(
        rows(&run.outcomes[0].outcome),
        [[Some("10000".to_owned()), Some("-30".to_owned())]]
    );
    assert_eq!(rows(&run.outcomes[1].outcome), [[Some("Köln".to_owned())]]);
    assert_eq!(
        rows(&run.outcomes[2].outcome),
        [[Some("Nut 'M6'".to_owned())]]
    );
}

#[tokio::test]
async fn scripts_transactions_errors_catalog_and_plans() {
    let Some(s) = session(false).await else {
        return;
    };
    let cleanup = "drop table if exists ide_line;\ndrop table if exists ide_item";
    let _ = s.run_script(cleanup, OnError::Continue).await;
    let setup = "create table ide_item (id int primary key, name nvarchar(40) not null, \
                 note nvarchar(20) constraint ide_item_note_df default 'none');\n\
                 create table ide_line (id int primary key, item_id int \
                 constraint ide_line_item_fk references ide_item (id), qty smallint);\n\
                 create index ide_line_qty on ide_line (qty);\n\
                 insert into ide_item (id, name) values (1, N'it''s; here'), (2, 'two');\n\
                 set nocount on;\n\
                 select id, name, note from ide_item order by id";
    let run = s.run_script(setup, OnError::Stop).await;
    assert_eq!(
        rows(&run.outcomes.last().unwrap().outcome),
        [
            [
                Some("1".into()),
                Some("it's; here".into()),
                Some("none".into())
            ],
            [Some("2".into()), Some("two".into()), Some("none".into())],
        ],
        "{:?}",
        run.outcomes
    );
    assert!(
        matches!(run.outcomes[3].outcome, Outcome::UpdateCount(2)),
        "{:?}",
        run.outcomes[3]
    );

    // A transaction ends with ROLLBACK and leaves nothing behind.
    let tx = s
        .run_script(
            "begin tran;\nupdate ide_item set name = 'x' where id = 1;\nrollback;\n\
             select name from ide_item where id = 1",
            OnError::Stop,
        )
        .await;
    assert_eq!(
        rows(&tx.outcomes[3].outcome),
        [[Some("it's; here".to_owned())]],
        "{:?}",
        tx.outcomes
    );

    // Errors carry SQL Server's message.
    let bad = s
        .run_script("select nosuch from ide_item", OnError::Stop)
        .await;
    match &bad.outcomes[0].outcome {
        Outcome::Error(e) => assert!(e.message.contains("nosuch"), "{e:?}"),
        other => panic!("{other:?}"),
    }

    let tables = introspect(&s).await.unwrap();
    let item = tables.iter().find(|t| t.name == "ide_item").unwrap();
    let line = tables.iter().find(|t| t.name == "ide_line").unwrap();
    assert_eq!(item.primary_key, ["id"]);
    let name = item.columns.iter().find(|c| c.name == "name").unwrap();
    assert!(!name.nullable, "{name:?}");
    assert!(name.data_type.starts_with("nvarchar"), "{name:?}");
    let note = item.columns.iter().find(|c| c.name == "note").unwrap();
    assert!(
        note.default
            .as_deref()
            .is_some_and(|d| d.contains("'none'")),
        "{note:?}"
    );
    assert_eq!(line.foreign_keys[0].referenced_table, "ide_item");
    assert_eq!(line.foreign_keys[0].columns, ["item_id"]);
    assert!(
        line.indexes
            .iter()
            .any(|i| i.name == "ide_line_qty" && !i.unique),
        "{:?}",
        line.indexes
    );
    let stock = tables.iter().find(|t| t.name == "stock").unwrap();
    assert_eq!(stock.primary_key, ["warehouse_id", "sku"]);
    assert_eq!(stock.foreign_keys.len(), 2, "{:?}", stock.foreign_keys);
    assert!(
        tables
            .iter()
            .any(|t| t.name == "stock_value" && t.kind == "view")
    );

    let plan = explain(&s, "select * from ide_line where qty > 3", false)
        .await
        .unwrap();
    assert!(!plan.nodes.is_empty(), "{plan:?}");
    let measured = explain(
        &s,
        "select s.sku, sum(m.delta) from movement m join stock s on s.sku = m.sku group by s.sku",
        true,
    )
    .await
    .unwrap();
    assert!(measured.analyzed, "{measured:?}");

    let _ = s.run_script(cleanup, OnError::Continue).await;
}

#[tokio::test]
async fn read_only_profiles_refuse_changes() {
    let Some(s) = session(true).await else {
        return;
    };
    let run = s
        .run_script(
            "select 1;\nset nocount on;\ndelete from item;\ncreate table ide_ro (x int)",
            OnError::Continue,
        )
        .await;
    assert_eq!(rows(&run.outcomes[0].outcome), [[Some("1".to_owned())]]);
    assert!(!matches!(run.outcomes[1].outcome, Outcome::Error(_)));
    assert!(matches!(run.outcomes[2].outcome, Outcome::Error(_)));
    assert!(matches!(run.outcomes[3].outcome, Outcome::Error(_)));
}

/// SPEC-0116 v2: an error in a statement of several lines points at the line SQL Server names.
#[tokio::test]
async fn multi_line_error_has_a_position() {
    let Some(s) = session(false).await else {
        return;
    };
    let script = "select 1 as one,\n  nosuch_column\nfrom sys.objects";
    let run = s.run_script(script, OnError::Stop).await;
    let offset = run.outcomes[0].error_offset.expect("position");
    assert!(script[offset..].starts_with("nosuch_column"), "{run:?}");
}
