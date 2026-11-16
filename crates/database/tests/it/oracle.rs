//! SPEC-0142 against a real Oracle database. Set `IDECRAFT_TEST_ORACLE` to a SPEC-0008 URL (e.g.
//! `oracle://idecraft@127.0.0.1:1521/FREEPDB1`) and the password in `IDECRAFT_TEST_ORACLE_PASSWORD`
//! or a file named by `IDECRAFT_TEST_ORACLE_PASSWORD_FILE` (`oracle.sh` of the reference environment);
//! without them these tests pass without doing anything. Each test works on its own tables, which it drops again.

use idecraft_database::{
    ConnectionProfile, Credentials, Dialect, OnError, Outcome, Secret, Session, explain, introspect,
};

async fn session(read_only: bool) -> Option<Session> {
    let url = std::env::var("IDECRAFT_TEST_ORACLE").ok()?;
    let password = std::env::var("IDECRAFT_TEST_ORACLE_PASSWORD")
        .ok()
        .or_else(|| {
            let file = std::env::var("IDECRAFT_TEST_ORACLE_PASSWORD_FILE").ok()?;
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
async fn scripts_with_plsql_transactions_and_catalog() {
    let Some(s) = session(false).await else {
        return;
    };
    assert_eq!(s.dialect(), Dialect::Oracle);
    let _ = s
        .run_script(
            "drop table ide_line purge;\ndrop table ide_item purge",
            OnError::Continue,
        )
        .await;
    let setup = "create table ide_item (id number(10) primary key, name varchar2(40) not null, \
                 note varchar2(20) default 'none');\n\
                 create table ide_line (id number(10) primary key, item_id number(10) \
                 constraint ide_line_item_fk references ide_item (id), qty number(5));\n\
                 create index ide_line_qty on ide_line (qty);\n\
                 insert into ide_item (id, name) values (1, q'[it's; here]');\n\
                 begin\n  insert into ide_item (id, name) values (2, 'two');\n  null;\nend;\n/\n\
                 select id, name, note from ide_item order by id";
    let run = s.run_script(setup, OnError::Stop).await;
    let last = run.outcomes.last().unwrap();
    assert_eq!(
        rows(&last.outcome),
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
    assert!(matches!(run.outcomes[3].outcome, Outcome::UpdateCount(1)));

    // A transaction opened for editing ends with ROLLBACK and leaves nothing behind.
    let tx = s
        .run_script(
            "set transaction read write;\nupdate ide_item set name = 'x' where id = 1;\nrollback;\n\
             select name from ide_item where id = 1",
            OnError::Stop,
        )
        .await;
    assert_eq!(
        rows(&tx.outcomes[3].outcome),
        [[Some("it's; here".to_owned())]]
    );

    // Errors carry Oracle's message and position.
    let bad = s
        .run_script("select nosuch from ide_item", OnError::Stop)
        .await;
    match &bad.outcomes[0].outcome {
        Outcome::Error(e) => {
            assert!(e.message.contains("ORA-00904"), "{e:?}");
            assert_eq!(e.position, Some(8));
        }
        other => panic!("{other:?}"),
    }

    let tables = introspect(&s).await.unwrap();
    let item = tables.iter().find(|t| t.name == "IDE_ITEM").unwrap();
    let line = tables.iter().find(|t| t.name == "IDE_LINE").unwrap();
    assert_eq!(item.primary_key, ["ID"]);
    let name = item.columns.iter().find(|c| c.name == "NAME").unwrap();
    assert_eq!(
        (name.data_type.as_str(), name.nullable),
        ("VARCHAR2(40)", false)
    );
    let note = item.columns.iter().find(|c| c.name == "NOTE").unwrap();
    assert_eq!(note.default.as_deref(), Some("'none'"));
    assert_eq!(line.foreign_keys[0].referenced_table, "IDE_ITEM");
    assert_eq!(line.foreign_keys[0].columns, ["ITEM_ID"]);
    assert!(
        line.indexes
            .iter()
            .any(|i| i.name == "IDE_LINE_QTY" && !i.unique)
    );

    let plan = explain(&s, "select * from ide_line where qty > 3", false)
        .await
        .unwrap();
    assert!(
        plan.nodes[0].operation.starts_with("SELECT STATEMENT"),
        "{plan:?}"
    );
    assert!(
        plan.nodes[0].children[0].object.is_some(),
        "{:?}",
        plan.nodes
    );

    let _ = s
        .run_script(
            "drop table ide_line purge;\ndrop table ide_item purge",
            OnError::Continue,
        )
        .await;
}

#[tokio::test]
async fn read_only_profiles_refuse_changes_and_blocks() {
    let Some(s) = session(true).await else {
        return;
    };
    let run = s
        .run_script(
            "select 1 from dual;\nbegin null; end;\n/\ncreate table ide_ro (x number)",
            OnError::Continue,
        )
        .await;
    assert_eq!(rows(&run.outcomes[0].outcome), [[Some("1".to_owned())]]);
    assert!(matches!(run.outcomes[1].outcome, Outcome::Error(_)));
    assert!(matches!(run.outcomes[2].outcome, Outcome::Error(_)));
}
