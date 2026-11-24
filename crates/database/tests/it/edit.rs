//! SPEC-0017 against SQLite (always) and PostgreSQL/MySQL when `FERRIS_IDEA_TEST_PG` /
//! `FERRIS_IDEA_TEST_MYSQL` / `FERRIS_IDEA_TEST_MARIADB` / `FERRIS_IDEA_TEST_SQLSERVER` are set. Each run works on its own table.

use crate::common;

use ferris_idea_database::{
    Applied, Column, ConnectionProfile, Credentials, EditSet, EditTarget, Failure, OnError,
    Outcome, Secret, Session, Table, Values,
};

fn table() -> Table {
    let column = |name: &str, nullable| Column {
        name: name.into(),
        data_type: "text".into(),
        nullable,
        default: None,
        generated: None,
        expression: None,
        identity: None,
        comment: None,
    };
    Table {
        storage: Vec::new(),
        hidden_columns: Vec::new(),
        name: "edit_probe".into(),
        kind: "table".into(),
        columns: vec![
            column("id", false),
            column("name", false),
            column("note", true),
        ],
        primary_key: vec!["id".into()],
        foreign_keys: vec![],
        indexes: vec![],
        unreadable: None,
        constraints: Vec::new(),
        comment: None,
    }
}

fn row(id: &str, name: &str, note: Option<&str>) -> Values {
    vec![
        ("id".into(), Some(id.into())),
        ("name".into(), Some(name.into())),
        ("note".into(), note.map(str::to_owned)),
    ]
}

async fn rows(s: &Session) -> Vec<Vec<Option<String>>> {
    let run = s
        .run_script(
            "select id, name, note from edit_probe order by id",
            OnError::Stop,
        )
        .await;
    match &run.outcomes[0].outcome {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("expected rows, got {other:?}"),
    }
}

fn strings(rows: &[[Option<&str>; 3]]) -> Vec<Vec<Option<String>>> {
    rows.iter()
        .map(|r| r.iter().map(|v| v.map(str::to_owned)).collect())
        .collect()
}

/// Runs AK-4 to AK-6 on a session where `edit_probe` does not exist yet.
async fn exercise(s: &Session, create: &str) {
    let setup = format!(
        "{create};\
         insert into edit_probe (id, name, note) values (1, 'Bolt', null), (2, 'Nut', 'M6'), (3, 'Pin', null)"
    );
    let run = s.run_script(&setup, OnError::Stop).await;
    assert!(
        run.outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{run:?}"
    );
    let target = EditTarget::new(&table(), s.dialect()).unwrap();

    // AK-4: update (with quote and backslash), insert, delete.
    let mut edits = EditSet::new(target.clone());
    edits
        .set_cell(&row("1", "Bolt", None), "note", Some(r"it's C:\tmp".into()))
        .unwrap();
    edits
        .insert_row(vec![
            ("id".into(), Some("4".into())),
            ("name".into(), Some("Washer".into())),
        ])
        .unwrap();
    edits.delete_row(&row("3", "Pin", None)).unwrap();
    assert_eq!(edits.apply(s).await, Applied::Committed(3));
    assert_eq!(
        rows(s).await,
        strings(&[
            [Some("1"), Some("Bolt"), Some(r"it's C:\tmp")],
            [Some("2"), Some("Nut"), Some("M6")],
            [Some("4"), Some("Washer"), None],
        ])
    );
    let after_first = rows(s).await;

    // AK-5: row 2 changed after loading; the batch's insert is rolled back too.
    let run = s
        .run_script(
            "update edit_probe set note = 'M8' where id = 2",
            OnError::Stop,
        )
        .await;
    assert_eq!(run.outcomes[0].outcome, Outcome::UpdateCount(1));
    let mut stale = EditSet::new(target.clone());
    stale
        .insert_row(vec![
            ("id".into(), Some("5".into())),
            ("name".into(), Some("Clip".into())),
        ])
        .unwrap();
    stale
        .set_cell(&row("2", "Nut", Some("M6")), "note", Some("M10".into()))
        .unwrap();
    assert_eq!(
        stale.apply(s).await,
        Applied::RolledBack {
            index: Some(1),
            failure: Failure::Conflict(0)
        }
    );
    let mut expected = after_first.clone();
    expected[1][2] = Some("M8".into());
    assert_eq!(rows(s).await, expected);

    // AK-6: a NOT NULL violation rolls back everything.
    let mut failing = EditSet::new(target);
    failing.delete_row(&row("4", "Washer", None)).unwrap();
    failing
        .set_cell(&row("1", "Bolt", Some(r"it's C:\tmp")), "name", None)
        .unwrap();
    match failing.apply(s).await {
        Applied::RolledBack {
            index: Some(1),
            failure: Failure::Error(e),
        } => assert!(e.sqlstate.is_some() || !e.message.is_empty(), "{e:?}"),
        other => panic!("expected an error at change 1, got {other:?}"),
    }
    assert_eq!(rows(s).await, expected);
}

#[tokio::test]
async fn sqlite_applies_and_rolls_back() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-edit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("edit.sqlite");
    let _ = std::fs::remove_file(&file);
    rusqlite::Connection::open(&file).unwrap();
    let profile =
        ConnectionProfile::from_url("edit", &format!("sqlite:{}", file.display())).unwrap();
    let s = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    exercise(
        &s,
        "create table edit_probe (id integer primary key, name text not null, note text)",
    )
    .await;
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

async fn server(var: &str) -> Option<Session> {
    let url = std::env::var(var).ok()?;
    let profile = ConnectionProfile::from_url("edit", &url).unwrap();
    Some(
        Session::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn postgres_applies_and_rolls_back() {
    let Some(s) = server("FERRIS_IDEA_TEST_PG").await else {
        return;
    };
    exercise(
        &s,
        "create temporary table edit_probe (id integer primary key, name text not null, note text)",
    )
    .await;
}

#[tokio::test]
async fn mysql_applies_and_rolls_back() {
    let Some(s) = server("FERRIS_IDEA_TEST_MYSQL").await else {
        return;
    };
    exercise(
        &s,
        "create temporary table edit_probe (id integer primary key, name varchar(50) not null, note varchar(50))",
    )
    .await;
}

#[tokio::test]
async fn mariadb_applies_and_rolls_back() {
    let Some(s) = server("FERRIS_IDEA_TEST_MARIADB").await else {
        return;
    };
    exercise(
        &s,
        "create temporary table edit_probe (id integer primary key, name varchar(50) not null, note varchar(50))",
    )
    .await;
}

#[tokio::test]
async fn sqlserver_applies_and_rolls_back() {
    // Same variables as tests/sqlserver.rs; `edit_probe` is a real table there, dropped again.
    let Some(url) = ferris_idea_core::product_var("FERRIS_IDEA_TEST_SQLSERVER").ok() else {
        return;
    };
    let password = ferris_idea_core::product_var("FERRIS_IDEA_TEST_SQLSERVER_PASSWORD")
        .unwrap_or_else(|_| {
            let file =
                ferris_idea_core::product_var("FERRIS_IDEA_TEST_SQLSERVER_PASSWORD_FILE").unwrap();
            std::fs::read_to_string(file).unwrap().trim().to_owned()
        });
    let profile = ConnectionProfile::from_url("edit", &url).unwrap();
    let s = Session::connect(&profile, &Credentials::Available(Secret::new(password)))
        .await
        .unwrap();
    let drop = "drop table if exists edit_probe";
    let _ = s.run_script(drop, OnError::Continue).await;
    exercise(
        &s,
        "create table edit_probe (id integer primary key, name nvarchar(50) not null, note nvarchar(50))",
    )
    .await;
    let _ = s.run_script(drop, OnError::Continue).await;
}
