//! SPEC-0010 against a real server. Set `IDECRAFT_TEST_MYSQL` to a SPEC-0008 URL of the reference
//! database (e.g. `mysql://idecraft@127.0.0.1:33069/inventory?ssl-mode=DISABLED`); without it these
//! tests pass without doing anything. The password comes from `IDECRAFT_TEST_MYSQL_PASSWORD` or the
//! file named by `IDECRAFT_TEST_MYSQL_PASSWORD_FILE` (`tests/common`).

use crate::common;

use idecraft_database::{ConnectionProfile, MySqlSession, OnError, Outcome};

async fn session(read_only: bool) -> Option<MySqlSession> {
    let url = std::env::var("IDECRAFT_TEST_MYSQL").ok()?;
    let mut profile = ConnectionProfile::from_url("reference", &url).unwrap();
    profile.read_only = read_only;
    Some(
        MySqlSession::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap(),
    )
}

fn values(outcome: &Outcome) -> Vec<Vec<Option<String>>> {
    match outcome {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("expected rows, got {other:?}"),
    }
}

fn error(outcome: &Outcome) -> (Option<String>, String) {
    match outcome {
        Outcome::Error(e) => (e.sqlstate.clone(), e.message.clone()),
        other => panic!("expected error, got {other:?}"),
    }
}

#[tokio::test]
async fn reference_counts_match() {
    let Some(s) = session(false).await else {
        return;
    };
    let run = s
        .run_script("select count(*), sum(delta) from movement", OnError::Stop)
        .await;
    assert_eq!(
        values(&run.outcomes[0].outcome),
        [[Some("10000".to_owned()), Some("-30".to_owned())]]
    );
}

#[tokio::test]
async fn two_selects_give_two_result_sets() {
    let Some(s) = session(false).await else {
        return;
    };
    let run = s
        .run_script("select 1 as a; select 'x;y' as b", OnError::Stop)
        .await;
    assert_eq!(run.outcomes.len(), 2);
    match &run.outcomes[1].outcome {
        Outcome::ResultSet { columns, rows } => {
            assert_eq!(columns, &["b"]);
            assert_eq!(rows, &[[Some("x;y".to_owned())]]);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn error_keeps_sqlstate_and_number() {
    let Some(s) = session(false).await else {
        return;
    };
    let run = s
        .run_script("select no_such_column from movement", OnError::Stop)
        .await;
    let (state, message) = error(&run.outcomes[0].outcome);
    assert_eq!(state.as_deref(), Some("42S22"));
    assert!(message.starts_with("MySQL error 1054:"), "{message}");
}

/// SPEC-0010 v2 AK-3: a syntax error points at the text the server stopped at.
#[tokio::test]
async fn syntax_error_has_a_position() {
    let Some(s) = session(false).await else {
        return;
    };
    let script = "select 1;\nselect *\n  frm movement";
    let run = s.run_script(script, OnError::Continue).await;
    let offset = run.outcomes[1].error_offset.expect("position");
    assert!(script[offset..].starts_with("frm movement"), "{run:?}");
}

#[tokio::test]
async fn insert_reports_update_count() {
    let Some(s) = session(false).await else {
        return;
    };
    let run = s
        .run_script(
            "create temporary table t (v int); insert into t values (1), (2), (3)",
            OnError::Stop,
        )
        .await;
    assert_eq!(run.outcomes[1].outcome, Outcome::UpdateCount(3));
}

#[tokio::test]
async fn read_only_profile_refuses_changes() {
    let Some(s) = session(true).await else { return };
    let run = s
        .run_script(
            "update movement set delta = delta where id = 1",
            OnError::Stop,
        )
        .await;
    assert_eq!(error(&run.outcomes[0].outcome).0.as_deref(), Some("25006"));
}

#[tokio::test]
async fn binary_values_are_hex() {
    let Some(s) = session(false).await else {
        return;
    };
    let run = s.run_script("select unhex('ff00')", OnError::Stop).await;
    assert_eq!(
        values(&run.outcomes[0].outcome),
        [[Some("0xff00".to_owned())]]
    );
}
