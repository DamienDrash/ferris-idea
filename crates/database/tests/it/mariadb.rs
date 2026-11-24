//! SPEC-0116 against a real MariaDB. Set `FERRIS_IDEA_TEST_MARIADB` to a SPEC-0008 URL of the
//! reference database (e.g. `mariadb://idecraft@127.0.0.1:33079/inventory?ssl-mode=DISABLED`); without
//! it these tests pass without doing anything. The password comes from
//! `FERRIS_IDEA_TEST_MARIADB_PASSWORD` or the file named by `FERRIS_IDEA_TEST_MARIADB_PASSWORD_FILE`
//! (`tests/common`).

use crate::common;

use ferris_idea_database::{ConnectionProfile, Dialect, OnError, Outcome, Session};

async fn session(read_only: bool) -> Option<Session> {
    let url = ferris_idea_core::product_var("FERRIS_IDEA_TEST_MARIADB").ok()?;
    let mut profile = ConnectionProfile::from_url("reference", &url).unwrap();
    profile.read_only = read_only;
    Some(
        Session::connect(&profile, &common::credentials(&profile))
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn reference_counts_and_dialect() {
    let Some(s) = session(false).await else {
        return;
    };
    assert_eq!(s.dialect(), Dialect::MariaDb);
    let run = s
        .run_script(
            "select count(*), sum(delta) from movement; select version() like '%MariaDB%'",
            OnError::Stop,
        )
        .await;
    match (&run.outcomes[0].outcome, &run.outcomes[1].outcome) {
        (Outcome::ResultSet { rows: counts, .. }, Outcome::ResultSet { rows: version, .. }) => {
            assert_eq!(
                counts,
                &[[Some("10000".to_owned()), Some("-30".to_owned())]]
            );
            assert_eq!(version, &[[Some("1".to_owned())]]);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn errors_name_mariadb_and_read_only_holds() {
    let Some(s) = session(true).await else {
        return;
    };
    let run = s
        .run_script("select nope from item; delete from item", OnError::Continue)
        .await;
    for outcome in &run.outcomes {
        match outcome.outcome {
            Outcome::Error(ref e) => assert!(e.message.starts_with("MariaDB error"), "{e:?}"),
            ref other => panic!("{other:?}"),
        }
    }
}

/// SPEC-0116 v2: MariaDB syntax errors point at the text the server stopped at.
#[tokio::test]
async fn syntax_error_has_a_position() {
    let Some(s) = session(false).await else {
        return;
    };
    let script = "select *\n  frm item";
    let run = s.run_script(script, OnError::Stop).await;
    let offset = run.outcomes[0].error_offset.expect("position");
    assert!(script[offset..].starts_with("frm item"), "{run:?}");
}
