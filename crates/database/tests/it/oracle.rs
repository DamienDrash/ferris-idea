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

/// SPEC-0142 v4 behaviour 6 (OBS-0083 L1): a read-only profile shows the plan of a query, also of
/// a change, without running it, and leaves no rows of its own in `PLAN_TABLE`.
#[tokio::test]
async fn read_only_profiles_explain() {
    let Some(writer) = session(false).await else {
        return;
    };
    let _ = writer
        .run_script("drop table ide_ro_plan purge", OnError::Continue)
        .await;
    let created = writer
        .run_script(
            "create table ide_ro_plan (id number primary key, qty number);\n\
             insert into ide_ro_plan values (1, 5)",
            OnError::Stop,
        )
        .await;
    assert!(
        created
            .outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{created:?}"
    );
    let s = session(true).await.unwrap();
    let plan = explain(&s, "select * from ide_ro_plan where id = 1", false)
        .await
        .unwrap();
    assert!(
        plan.nodes[0].operation.starts_with("SELECT STATEMENT"),
        "{plan:?}"
    );
    let plan = explain(&s, "delete from ide_ro_plan", false).await.unwrap();
    assert!(
        plan.nodes[0].operation.starts_with("DELETE STATEMENT"),
        "{plan:?}"
    );
    let left = s
        .run_script(
            "select count(*) from ide_ro_plan;\n\
             select count(*) from plan_table where statement_id = 'IDECRAFT'",
            OnError::Stop,
        )
        .await;
    assert_eq!(rows(&left.outcomes[0].outcome), [[Some("1".to_owned())]]);
    assert_eq!(rows(&left.outcomes[1].outcome), [[Some("0".to_owned())]]);
    let _ = writer
        .run_script("drop table ide_ro_plan purge", OnError::Continue)
        .await;
}

/// SPEC-0142 v4 behaviour 5 (OBS-0083 L2): an identity column is read as its clause, not as a
/// default naming the system sequence, and the generated DDL runs on Oracle again.
#[tokio::test]
async fn identity_columns_and_their_ddl() {
    let Some(s) = session(false).await else {
        return;
    };
    let _ = s
        .run_script(
            "drop table ide_ident purge;\ndrop table ide_ident2 purge",
            OnError::Continue,
        )
        .await;
    let created = s
        .run_script(
            "create table ide_ident (id number generated by default on null as identity primary key, \
             state varchar2(10) default 'new' not null);\n\
             comment on column ide_ident.state is 'the state'",
            OnError::Stop,
        )
        .await;
    assert!(
        created
            .outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{created:?}"
    );
    let tables = introspect(&s).await.unwrap();
    let table = tables.iter().find(|t| t.name == "IDE_IDENT").unwrap();
    let id = &table.columns[0];
    assert_eq!(
        (id.identity.as_deref(), id.default.as_deref()),
        (Some("GENERATED BY DEFAULT ON NULL AS IDENTITY"), None)
    );
    assert_eq!(table.columns[1].comment.as_deref(), Some("the state"));
    let ddl = idecraft_database::object_ddl(&tables, &[], "IDE_IDENT", Dialect::Oracle)
        .unwrap()
        .replace("\"IDE_IDENT\"", "\"IDE_IDENT2\"");
    let copied = s.run_script(&ddl, OnError::Stop).await;
    assert!(
        copied
            .outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{ddl}\n{copied:?}"
    );
    let _ = s
        .run_script(
            "drop table ide_ident purge;\ndrop table ide_ident2 purge",
            OnError::Continue,
        )
        .await;
}

/// SPEC-0142 v4 behaviour 5 (OBS-0083 L5): procedures, functions and triggers of the user's schema
/// are listed with their definitions; the sequence behind an identity column is not listed.
#[tokio::test]
async fn routines_triggers_and_sequences() {
    let Some(s) = session(false).await else {
        return;
    };
    let drop = "drop table ide_obj purge;\ndrop procedure ide_obj_p;\ndrop function ide_obj_f";
    let _ = s.run_script(drop, OnError::Continue).await;
    let created = s
        .run_script(
            "create table ide_obj (id number generated always as identity primary key, n number);\n\
             create or replace procedure ide_obj_p as begin null; end;\n/\n\
             create or replace function ide_obj_f (x number) return number as begin return x + 1; end;\n/\n\
             create or replace trigger ide_obj_trg before insert or update on ide_obj \
             for each row begin null; end;\n/",
            OnError::Stop,
        )
        .await;
    assert!(
        created
            .outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{created:?}"
    );
    let objects = idecraft_database::schema_objects(&s, None).await.unwrap();
    let find = |name: &str| objects.iter().find(|o| o.name == name).cloned();
    assert_eq!(find("IDE_OBJ_P").unwrap().kind, "procedure");
    assert_eq!(find("IDE_OBJ_F").unwrap().kind, "function");
    let trigger = find("IDE_OBJ_TRG").unwrap();
    assert_eq!(
        (trigger.kind.as_str(), trigger.table.as_deref()),
        ("trigger", Some("IDE_OBJ"))
    );
    assert!(
        !objects.iter().any(|o| o.name.starts_with("ISEQ$$")),
        "{objects:?}"
    );
    let definition = idecraft_database::object_definition(&s, None, &find("IDE_OBJ_F").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(definition.starts_with("CREATE OR REPLACE"), "{definition}");
    assert!(definition.contains("FUNCTION"), "{definition}");
    assert!(definition.contains("IDE_OBJ_F"), "{definition}");
    let _ = s.run_script(drop, OnError::Continue).await;
}
