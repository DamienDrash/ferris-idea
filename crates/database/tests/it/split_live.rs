//! SPEC-0005 v3 AK-18: the engines agree with the splitter. Scripts that use escape strings,
//! nested comments, SQLite brackets, MySQL's `--` rule and `DELIMITER` run statement by statement
//! without an error: SQLite and DuckDB in the test process, PostgreSQL, MySQL and MariaDB where
//! `FERRIS_IDEA_TEST_PG`, `FERRIS_IDEA_TEST_MYSQL` and `FERRIS_IDEA_TEST_MARIADB` are set.

use crate::common;

use ferris_idea_database::{ConnectionProfile, Dialect, OnError, Outcome, Session};

async fn runs(url: &str, script: &str, statements: usize) {
    let profile = ConnectionProfile::from_url("reference", url).unwrap();
    let session = Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap();
    let run = session.run_script(script, OnError::Stop).await;
    assert_eq!(run.outcomes.len(), statements, "{url}: {script}");
    for o in &run.outcomes {
        assert!(
            !matches!(o.outcome, Outcome::Error(_)),
            "{url}: {script}: {:?}",
            o.outcome
        );
    }
}

fn script_for(dialect: Dialect, tag: u32) -> (String, usize) {
    match dialect {
        Dialect::PostgreSql | Dialect::DuckDb => (
            "select E'it\\'s; x';\n/* a /* b */ ; */ select 2;\n".into(),
            2,
        ),
        Dialect::Sqlite => ("select [a;b] from (select 1 as [a;b]); select 2;".into(), 2),
        Dialect::MySql | Dialect::MariaDb => (
            format!(
                "select 5--1;\n-- a comment; with a semicolon\nselect 2;\n\
                 drop procedure if exists rf_split{tag};\n\
                 DELIMITER //\n\
                 create procedure rf_split{tag}()\nbegin\n  select 1;\n  select ';';\nend //\n\
                 DELIMITER ;\n\
                 call rf_split{tag}();\n\
                 drop procedure rf_split{tag};\n"
            ),
            // The `CALL` gives both result sets of the procedure (SPEC-0005 v4 behaviour 18).
            7,
        ),
        Dialect::SqlServer | Dialect::Oracle => unreachable!(),
    }
}

#[tokio::test]
async fn engines_agree_with_the_splitter() {
    let tag = std::process::id();
    let dir = std::env::temp_dir().join(format!("ferris-idea-split-live-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lite = dir.join("s.sqlite");
    rusqlite::Connection::open(&lite).unwrap();
    let duck = dir.join("s.duckdb");
    duckdb::Connection::open(&duck).unwrap();
    let mut urls = vec![
        (format!("sqlite:{}", lite.display()), Dialect::Sqlite),
        (format!("duckdb:{}", duck.display()), Dialect::DuckDb),
    ];
    for (var, dialect) in [
        ("FERRIS_IDEA_TEST_PG", Dialect::PostgreSql),
        ("FERRIS_IDEA_TEST_MYSQL", Dialect::MySql),
        ("FERRIS_IDEA_TEST_MARIADB", Dialect::MariaDb),
    ] {
        if let Ok(url) = std::env::var(var) {
            urls.push((url, dialect));
        }
    }
    for (url, dialect) in urls {
        let (script, count) = script_for(dialect, tag);
        runs(&url, &script, count).await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0005 v4 AK-19 (OBS-0051 L2): on MySQL and MariaDB, where `FERRIS_IDEA_TEST_MYSQL` and
/// `FERRIS_IDEA_TEST_MARIADB` are set, one `CALL` of a procedure with two `SELECT`s gives both result
/// sets in order, all with the statement's range; a procedure without one gives its update count.
#[tokio::test]
async fn a_call_returns_every_result_set() {
    let tag = std::process::id();
    for var in ["FERRIS_IDEA_TEST_MYSQL", "FERRIS_IDEA_TEST_MARIADB"] {
        let Ok(url) = std::env::var(var) else {
            eprintln!("skipped: {var} not set");
            continue;
        };
        let profile = ConnectionProfile::from_url("reference", &url).unwrap();
        let session = Session::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap();
        let script = format!(
            "drop procedure if exists rf_sets{tag};\n\
             drop procedure if exists rf_none{tag};\n\
             DELIMITER //\n\
             create procedure rf_sets{tag}()\nbegin\n  select 1 as a;\n  select 'x' as b, 2 as c;\nend //\n\
             create procedure rf_none{tag}()\nbegin\n  set @v = 1;\nend //\n\
             DELIMITER ;\n"
        );
        let made = session.run_script(&script, OnError::Stop).await;
        assert!(
            made.outcomes
                .iter()
                .all(|o| !matches!(o.outcome, Outcome::Error(_))),
            "{var}: {:?}",
            made.outcomes
        );
        let call = format!("call rf_sets{tag}()");
        let run = session.run_script(&call, OnError::Stop).await;
        let sets: Vec<_> = run
            .outcomes
            .iter()
            .map(|o| match &o.outcome {
                Outcome::ResultSet { columns, rows } => (columns.clone(), rows.clone()),
                other => panic!("{var}: not a result set: {other:?}"),
            })
            .collect();
        assert_eq!(
            sets,
            [
                (vec!["a".to_owned()], vec![vec![Some("1".to_owned())]]),
                (
                    vec!["b".to_owned(), "c".to_owned()],
                    vec![vec![Some("x".to_owned()), Some("2".to_owned())]]
                ),
            ],
            "{var}"
        );
        assert!(
            run.outcomes
                .iter()
                .all(|o| o.range == run.outcomes[0].range)
        );
        let none = session
            .run_script(&format!("call rf_none{tag}()"), OnError::Stop)
            .await;
        assert_eq!(none.outcomes.len(), 1, "{var}");
        assert!(
            matches!(none.outcomes[0].outcome, Outcome::UpdateCount(_)),
            "{var}: {:?}",
            none.outcomes[0].outcome
        );
        let _ = session
            .run_script(
                &format!("drop procedure rf_sets{tag}; drop procedure rf_none{tag};"),
                OnError::Continue,
            )
            .await;
    }
}
