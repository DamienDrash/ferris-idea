//! SPEC-0093 v3 AK-7 and SPEC-0108 v4 AK-13: cancelling a running statement, the startup script and
//! the statement time limit. SQLite always; PostgreSQL and MySQL live with `IDECRAFT_TEST_PG` /
//! `IDECRAFT_TEST_MYSQL` (skipped without them).

use crate::common;

use std::time::{Duration, Instant};

use idecraft_database::{
    ConnectionProfile, Credentials, OnError, Outcome, ScriptRun, Session, SessionError,
};

fn live(var: &str) -> Option<ConnectionProfile> {
    let url = std::env::var(var).ok()?;
    Some(ConnectionProfile::from_url("live", &url).unwrap())
}

/// Runs `sql` and cancels it after 300 ms from another task; returns the run and how long it took.
async fn cancelled(session: &Session, sql: &str) -> (ScriptRun, Duration) {
    let handle = session.cancel_handle().expect("a cancel handle");
    let started = Instant::now();
    let (run, cancel) = tokio::join!(session.run_script(sql, OnError::Stop), async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        handle.cancel().await
    });
    cancel.expect("cancel request");
    (run, started.elapsed())
}

fn first(run: &ScriptRun) -> &Outcome {
    &run.outcomes.first().expect("one outcome").outcome
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_statement_is_interrupted() {
    let file = std::env::temp_dir().join(format!("idecraft-cancel-{}.db", std::process::id()));
    std::fs::write(&file, b"").unwrap();
    let p = ConnectionProfile::from_url("lite", &format!("sqlite:{}", file.display())).unwrap();
    let s = Session::connect(&p, &Credentials::NotNeeded).await.unwrap();
    let (run, took) = cancelled(
        &s,
        "with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c",
    )
    .await;
    assert!(matches!(first(&run), Outcome::Error(_)), "{run:?}");
    assert!(took < Duration::from_secs(10), "{took:?}");
    drop(s);
    let _ = std::fs::remove_file(&file);
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_cancel_startup_script_and_time_limit() {
    let Some(p) = live("IDECRAFT_TEST_PG") else {
        return;
    };
    let c = common::credentials(&p);
    let s = Session::connect(&p, &c).await.unwrap();
    let (run, took) = cancelled(&s, "select pg_sleep(30)").await;
    match first(&run) {
        Outcome::Error(e) => assert_eq!(e.sqlstate.as_deref(), Some("57014"), "{e}"),
        other => panic!("{other:?}"),
    }
    assert!(took < Duration::from_secs(10), "{took:?}");
    drop(s);

    // The startup script runs before anything else; a failing one stops the connection.
    let mut started = p.clone();
    started.options.startup_script = Some("set application_name = 'idecraft-startup'".into());
    let s = Session::connect(&started, &c).await.unwrap();
    let run = s.run_script("show application_name", OnError::Stop).await;
    match first(&run) {
        Outcome::ResultSet { rows, .. } => {
            assert_eq!(rows[0][0].as_deref(), Some("idecraft-startup"))
        }
        other => panic!("{other:?}"),
    }
    drop(s);
    started.options.startup_script = Some("select * from no_such_table_here".into());
    let err = Session::connect(&started, &c).await.err().unwrap();
    assert!(matches!(err, SessionError::Startup { .. }), "{err}");
    // A profile of an untrusted project does not run its startup script (SPEC-0278 safe mode).
    started.options.startup_allowed = false;
    assert!(Session::connect(&started, &c).await.is_ok());

    // The server ends a statement that runs past the limit.
    let mut limited = p.clone();
    limited.options.query_timeout_secs = Some(1);
    let s = Session::connect(&limited, &c).await.unwrap();
    let t = Instant::now();
    let run = s.run_script("select pg_sleep(5)", OnError::Stop).await;
    assert!(matches!(first(&run), Outcome::Error(_)), "{run:?}");
    assert!(t.elapsed() < Duration::from_secs(4));
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_cancel_and_time_limit() {
    let Some(p) = live("IDECRAFT_TEST_MYSQL") else {
        return;
    };
    let c = common::credentials(&p);
    let s = Session::connect(&p, &c).await.unwrap();
    let (_, took) = cancelled(&s, "select sleep(30)").await;
    assert!(
        took < Duration::from_secs(10),
        "KILL QUERY ends SLEEP early: {took:?}"
    );
    // The session stays usable after the cancel.
    let run = s.run_script("select 1", OnError::Stop).await;
    assert!(matches!(first(&run), Outcome::ResultSet { .. }), "{run:?}");
    drop(s);
    let mut limited = p.clone();
    limited.options.query_timeout_secs = Some(1);
    let s = Session::connect(&limited, &c).await.unwrap();
    let t = Instant::now();
    let _ = s.run_script("select sleep(5)", OnError::Stop).await;
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
}
