//! SPEC-0009 against a real server. Set `IDECRAFT_TEST_PG` to a SPEC-0008 URL of the reference
//! database (e.g. `postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable`); without it
//! these tests pass without doing anything. The password comes from `IDECRAFT_TEST_PG_PASSWORD` or
//! the file named by `IDECRAFT_TEST_PG_PASSWORD_FILE` (`tests/common`).

use crate::common;

use idecraft_database::{ConnectionProfile, OnError, Outcome, PgSession};

async fn session(read_only: bool) -> Option<PgSession> {
    let url = std::env::var("IDECRAFT_TEST_PG").ok()?;
    let mut profile = ConnectionProfile::from_url("reference", &url).unwrap();
    profile.read_only = read_only;
    Some(
        PgSession::connect(&profile, &common::credentials(&profile))
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

fn sqlstate(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Error(e) => e.sqlstate.clone(),
        _ => None,
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
async fn error_keeps_sqlstate_and_position() {
    let Some(s) = session(false).await else {
        return;
    };
    let script = "select 1;\nselect no_such_column from movement";
    let run = s.run_script(script, OnError::Stop).await;
    let failed = &run.outcomes[1];
    assert_eq!(sqlstate(&failed.outcome).as_deref(), Some("42703"));
    let offset = failed.error_offset.expect("position mapped");
    assert!(script[offset..].starts_with("no_such_column"));
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
    assert_eq!(sqlstate(&run.outcomes[0].outcome).as_deref(), Some("25006"));
}

/// SPEC-0093 v8 AK-15 (OBS-0068 L1): a `RAISE NOTICE` in a `DO` block reaches the statement that
/// raised it, and only that one.
#[tokio::test]
async fn notices_belong_to_their_statement() {
    let Some(s) = session(true).await else {
        return;
    };
    let run = s
        .run_script(
            "do $pd$ begin raise notice 'inside; the block'; raise warning 'second'; end $pd$; select 1",
            OnError::Stop,
        )
        .await;
    assert_eq!(run.outcomes.len(), 2, "{run:?}");
    assert_eq!(
        run.outcomes[0].notices,
        ["NOTICE: inside; the block", "WARNING: second"]
    );
    assert!(run.outcomes[1].notices.is_empty(), "{run:?}");
}
