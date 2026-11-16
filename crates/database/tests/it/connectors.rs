//! SPEC-0300 against real servers of protocol-family systems (ADR-0020). Each system is tested
//! when `IDECRAFT_TEST_<X>` holds its URL and `IDECRAFT_TEST_<X>_PASSWORD` or
//! `IDECRAFT_TEST_<X>_PASSWORD_FILE` its password (`connectors.sh env <system>` of the reference environment
//! prints them); without them the test passes without doing anything. Every run works in a schema or
//! database of its own (`idc_conn_<pid>`) and drops it again.

use idecraft_database::{
    ConnectionProfile, Credentials, Dialect, OnError, Outcome, Secret, Session, current_schema,
    explain, introspect_schema, list_schemas, test_connection, view_definitions,
};

/// The profile and credentials of system `var`, if configured.
fn configured(var: &str) -> Option<(ConnectionProfile, Credentials)> {
    let url = std::env::var(var).ok().filter(|u| !u.is_empty())?;
    let password = std::env::var(format!("{var}_PASSWORD"))
        .ok()
        .filter(|p| !p.is_empty())
        .or_else(|| {
            let file = std::env::var(format!("{var}_PASSWORD_FILE")).ok()?;
            Some(
                std::fs::read_to_string(&file)
                    .unwrap_or_else(|e| panic!("{var}_PASSWORD_FILE {file}: {e}"))
                    .trim()
                    .to_owned(),
            )
        });
    let profile = ConnectionProfile::from_url("family-system", &url).unwrap();
    let credentials = password.map_or(Credentials::NotNeeded, |p| {
        Credentials::Available(Secret::new(p))
    });
    Some((profile, credentials))
}

async fn run(session: &Session, sql: &str) -> Vec<Vec<Option<String>>> {
    let run = session.run_script(sql, OnError::Stop).await;
    let mut last = Vec::new();
    for o in run.outcomes {
        match o.outcome {
            Outcome::Error(e) => panic!("{sql}: {e}"),
            Outcome::ResultSet { rows, .. } => last = rows,
            _ => {}
        }
    }
    last
}

/// Creates two related tables, an index and a view in a schema of their own, then reads them back
/// through the connector's catalogue, lists schemas, explains a query and drops everything.
async fn exercise(var: &str, system: &str, family: Dialect) {
    let Some((profile, credentials)) = configured(var) else {
        return;
    };
    assert_eq!(profile.system.as_deref(), Some(system));
    assert_eq!(profile.dialect, family);
    let note = test_connection(&profile, &credentials).await;
    assert!(note.is_ok(), "{system}: {note:?}");
    let s = Session::connect(&profile, &credentials).await.unwrap();
    assert_eq!(s.system().map(|c| c.id.as_str()), Some(system));
    let version = s
        .system()
        .and_then(|c| c.sql.version_query.clone())
        .unwrap_or_else(|| "select version()".into());
    let version = run(&s, &version).await;
    eprintln!("{system} server version: {:?}", version.first());

    let schema = format!("idc_conn_{}", std::process::id());
    let mysql = family.mysql_like();
    let (create, enter, drop) = if mysql {
        (
            format!("create database {schema}"),
            format!("use {schema}"),
            format!("drop database {schema}"),
        )
    } else {
        (
            format!("create schema {schema}"),
            format!("set search_path to {schema}"),
            format!("drop schema {schema} cascade"),
        )
    };
    let _ = s.run_script(&drop, OnError::Stop).await;
    run(&s, &create).await;
    run(&s, &enter).await;
    let body = async {
        run(
            &s,
            "create table parent (id int primary key, name varchar(40) not null);\n\
             create table child (id int primary key, parent_id int not null, \
             note varchar(20) default 'x', \
             constraint child_parent foreign key (parent_id) references parent (id));\n\
             create index child_note on child (note);\n\
             create view child_ids as select id from child;\n\
             insert into parent values (1, 'a'), (2, 'b');\n\
             insert into child values (10, 1, 'n'), (11, 2, null)",
        )
        .await;
        assert_eq!(current_schema(&s).await.unwrap(), schema);
        let schemas = list_schemas(&s).await.unwrap();
        assert!(
            schemas.iter().any(|x| x.name == schema && x.current),
            "{system}: {schemas:?}"
        );
        let tables = introspect_schema(&s, Some(&schema)).await.unwrap();
        let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&"parent") && names.contains(&"child"),
            "{system}: {names:?}"
        );
        let child = tables.iter().find(|t| t.name == "child").unwrap();
        assert_eq!(child.primary_key, ["id"], "{system}");
        let columns: Vec<&str> = child.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(columns, ["id", "parent_id", "note"], "{system}");
        assert!(!child.columns[1].nullable, "{system}");
        assert_eq!(
            child.foreign_keys.len(),
            1,
            "{system}: {:?}",
            child.foreign_keys
        );
        assert_eq!(child.foreign_keys[0].referenced_table, "parent");
        assert!(
            child.indexes.iter().any(|i| i.name == "child_note"),
            "{system}: {:?}",
            child.indexes
        );
        let views = view_definitions(&s).await.unwrap();
        assert!(
            views.iter().any(|v| v.name == "child_ids"),
            "{system}: {views:?}"
        );
        let plan = explain(&s, "select * from child where parent_id = 1", false)
            .await
            .unwrap_or_else(|e| panic!("{system}: {e}"));
        assert!(!plan.nodes.is_empty(), "{system}: {plan:?}");
        let measured = explain(&s, "update child set note = 'm' where id = 10", true)
            .await
            .unwrap_or_else(|e| panic!("{system}: {e}"));
        assert!(measured.analyzed && measured.rolled_back, "{system}");
        let note = run(&s, "select note from child where id = 10").await;
        assert_eq!(
            note,
            [[Some("n".to_owned())]],
            "{system}: measuring was rolled back"
        );
    };
    body.await;
    run(&s, &drop).await;
}

#[tokio::test]
async fn timescaledb() {
    exercise(
        "IDECRAFT_TEST_TIMESCALE",
        "timescaledb",
        Dialect::PostgreSql,
    )
    .await;
}

#[tokio::test]
async fn tidb() {
    exercise("IDECRAFT_TEST_TIDB", "tidb", Dialect::MySql).await;
}

#[tokio::test]
async fn yugabytedb() {
    exercise("IDECRAFT_TEST_YUGABYTE", "yugabytedb", Dialect::PostgreSql).await;
}

#[tokio::test]
async fn cockroachdb() {
    exercise(
        "IDECRAFT_TEST_COCKROACH",
        "cockroachdb",
        Dialect::PostgreSql,
    )
    .await;
}
